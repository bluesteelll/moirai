//! The toy log as the crash enumerator's subject (WP-40, E4): scenarios of processes and operations, the ledger of every
//! effect they attempt, acknowledge and observe, and the recovery a crash or a death is followed by.
//!
//! Every scenario runs the toy with the seeded bugs its subject names; with none it must pass every dimension of the
//! enumerator (the toy follows [F16]), and with one bug on the enumerator must report a violation of that bug's class.
//!
//! The enumerator's own verdicts (the ledger's first read and recovered state, the simulator's protocol-violation
//! checks) are R-HARN-S's. Every check the subject adds to them is in [`checks`], whose header states their authorship
//! under PLAN §3.1 S4 and its open disposition (WP-40 review, finding 2); this file only runs the scenarios, feeds the
//! ledger and calls those checks and the toy's own `doctor --verify`.
#![allow(dead_code)]

mod checks;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use moirai_toylog::{
    Bugs, ClaimOp, CommitOp, Config, DONE_TAG, FileOp, ForkOp, MAIN, Op, PROBE_REF, ProcLocks,
    ReleaseOp, RuntimeOp, State, Toy, ToyError, file_name, init::image, verify,
};
use moirai_vfs::{ProcHost, RootAccess, RootRole, StoreFs};
use moirai_vfs_sim::enumerate::{
    Class, EffectKey, EffectKind, EffectSet, EffectWrite, Ledger, Recovered, Subject,
};
use moirai_vfs_sim::{
    CrashPlan, Event, FaultRates, SimConfig, SimVfs, SimWorld, TaskEnd, VolumeProfile,
};

use checks::{
    AckedGroup, Found, ReadWatch, SimTap, T_TASK, chain_breaks, head_may_be_damaged,
    intent_namespace, ns_location, outcome_unknown, read_all, reader_refusal, trace_violations,
    uncovered,
};

/// The store directory.
pub const STORE: &str = "/sim/store";
/// The project directories, by index (1-based in the toy).
pub const PROJ: [&str; 3] = ["/sim/proj/a", "/sim/proj/b", "/sim/vol2/c"];
/// The mount point of the second volume (cross-volume moves).
pub const VOL2: &str = "/sim/vol2";

/// `EffectKind::Other` codes.
pub const ORPH: u16 = 1;
/// The namespace location of a project file.
pub const NS: u16 = 2;
/// A fork's pin.
pub const PIN: u16 = 3;
/// `HEAD.flags.quiet`.
pub const FLAG: u16 = 4;
/// A lazy runtime row.
pub const RT: u16 = 5;

/// Intent states as effect values.
pub const OPEN: u64 = 1;
/// Done.
pub const DONE: u64 = 2;
/// Aborted.
pub const ABORTED: u64 = 3;
/// A file in the store's trash.
pub const TRASH: u64 = 0x20;
/// A file in two places, or with other content.
pub const BAD: u64 = 0x30;

/// The op-id bit of a commit's ref move (a separate ledger operation, [F16] P-69).
pub const REF_BIT: u64 = 1 << 62;
/// The op-id bit of an intent's abort outcome.
pub const ABORT_BIT: u64 = 1 << 61;
/// The op-id bit of an intent's namespace change.
pub const NS_BIT: u64 = 1 << 60;
/// The recovering writer's probe operation.
pub const PROBE_OP: u64 = 0x7FFF_0000_0000_0001;
/// The seed of the scratch world a prefill runs in.
pub const PREFILL_SEED: u64 = 0x9E1F_1111;
/// The `ref_old` an import carries: never a tip, so every import is parked (I27′).
pub const STALE: u64 = 0x0DEA_DBEE_F000_0000;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The digest of an operation's content.
pub fn dig(op: u64) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for b in op.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h | 1
}

/// The content of project file `f`.
pub fn content(f: u64) -> Vec<u8> {
    format!("the content of project file {f}\n").into_bytes()
}

/// The location code of a file in project directory `idx`.
pub fn at_dir(idx: u8) -> u64 {
    0x10 + u64::from(idx)
}

// ---------------------------------------------------------------------------------------------------------------------
// Scenarios

/// One step of a process's task.
#[derive(Clone, Debug)]
pub enum Act {
    /// A commit on `ref_name` creating `creates`, completing `completes`, with an idempotency key when `key`.
    Commit {
        op: u64,
        ref_name: u64,
        creates: Vec<u64>,
        completes: Option<u64>,
        key: bool,
        filler: u32,
    },
    /// An import onto `ref_name` with a stale `ref_old`: always parked (I27′).
    Import { op: u64, ref_name: u64 },
    /// A lease claim.
    Claim { op: u64, uid: u64 },
    /// A lease release.
    Release { op: u64, uid: u64 },
    /// A fork of `from` named `name`.
    Fork { op: u64, from: u64, name: u64 },
    /// A lazy runtime batch.
    Runtime {
        op: u64,
        rows: Vec<(u64, u64)>,
        pad: u32,
        symbols: Vec<String>,
        target: u64,
    },
    /// A maintenance run.
    Checkpoint,
    /// `file mv` of `file` from `src` to `dst`.
    Mv {
        key: u64,
        file: u64,
        src: u8,
        dst: u8,
    },
    /// `file rm` of `file` in `src`, to the trash when `trash`.
    Rm {
        key: u64,
        file: u64,
        src: u8,
        trash: bool,
    },
    /// `config set`'s `config_gen` bump.
    Bump,
    /// `quiet on` / `quiet off`.
    Quiet { op: u64, on: bool },
    /// Take a quiet byte for the rest of the task.
    HoldQuiet,
    /// Release the quiet byte taken.
    ReleaseQuiet,
    /// Read `views` times and observe everything seen.
    Read { views: usize },
    /// `doctor --verify`: the invariants, the acknowledged groups' chain, the pinned files.
    Doctor,
    /// A flush holder's pass with no group of its own.
    Adopt,
    /// Intent recovery.
    RecoverIntents,
    /// `repair`: the sealed-file check and a rebuild of a damaged segment.
    Repair,
}

/// One simulated process: its tasks (several tasks are several clients of one process, [F16] P-3).
#[derive(Clone, Debug)]
pub struct ProcPlan {
    pub name: String,
    /// `Some(false)`: Unknown-boot mode.
    pub known: Option<bool>,
    pub tasks: Vec<Vec<Act>>,
    /// A wall-clock step applied to this process before its tasks start (FM-7.1), in ms.
    pub wall_step_ms: i64,
}

impl ProcPlan {
    pub fn new(name: &str, acts: Vec<Act>) -> ProcPlan {
        ProcPlan {
            name: name.to_owned(),
            known: Some(true),
            tasks: vec![acts],
            wall_step_ms: 0,
        }
    }

    pub fn unknown_boot(mut self) -> ProcPlan {
        self.known = Some(false);
        self
    }

    pub fn with_tasks(name: &str, tasks: Vec<Vec<Act>>) -> ProcPlan {
        ProcPlan {
            name: name.to_owned(),
            known: Some(true),
            tasks,
            wall_step_ms: 0,
        }
    }
}

/// What the driver does before a phase.
#[derive(Clone, Debug)]
pub enum DriverAct {
    /// Let virtual time pass (dead holders' bytes are released, deadlines pass).
    Advance(u64),
    /// A system crash with every unflushed write reverted (a scripted reboot inside the scenario).
    Crash,
}

/// One phase: driver actions, then processes run to their end.
#[derive(Clone, Debug, Default)]
pub struct Phase {
    pub before: Vec<DriverAct>,
    pub procs: Vec<ProcPlan>,
}

/// How the store exists before the workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreSetup {
    /// `init`'s image, with the first boot's identity (Known).
    Image,
    /// `init`'s image with a zero `boot_id` (an `init` in Unknown-boot mode).
    ImageUnknownBoot,
    /// No store: the workload runs `init`.
    None,
}

/// A setup-injected extent file ([`Scenario::stray`]): a `log.<n>` under a number the prefilled store no longer uses (its
/// extent was retired and deleted), which no process of a live store writes ([F16] P-74, P-75). `repair` without a valid
/// `HEAD` slot starts its scan at the head of the lowest extent of the epoch from which every later extent exists (P-85
/// step 2, P-97), the one scan start whose chain value comes from the record itself: only that head's position and epoch
/// checks (P-54, P-55) keep such a file out of the rebuilt state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stray {
    /// A byte copy of the prefilled `log.<from>` placed as `log.<to>`: a file copied or renamed to another number.
    Copy { from: u32, to: u32 },
    /// `log.1` of the store's own configuration and identity under another epoch: the first extent of an `init` image
    /// whose epoch is [`FOREIGN_EPOCH`].
    Foreign,
}

/// The epoch of `init::image` in every scenario's store.
pub const EPOCH: u64 = 0x5EED_E90C_0000_0001;
/// The epoch of [`Stray::Foreign`]'s extent.
pub const FOREIGN_EPOCH: u64 = 0x5EED_E90C_0000_0002;
/// The store identity of `init::image` in every scenario's store.
pub const STORE_ID: [u8; 16] = [7; 16];
/// The wall clock of `init::image`, in ms.
pub const INIT_WALL_MS: i64 = 1_790_000_000_000;

/// A scenario.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub cfg: Config,
    pub store: StoreSetup,
    /// Project files: (file, directory index).
    pub files: Vec<(u64, u8)>,
    pub phases: Vec<Phase>,
    pub tracked_refs: Vec<u64>,
    pub rates: FaultRates,
    pub trace: bool,
    /// The store is created by `init` in the workload's first process (P-24, P-88).
    pub init_in_workload: bool,
    /// Virtual time per scheduling point, in ns (the simulator's `tick_ns`).
    pub tick_ns: u64,
    /// Phases that build the store before the workload, run once per boot identity in a scratch world whose store files
    /// are then placed durably ([`SimWorld::put_file`]): the history a scenario starts from, not enumerated itself.
    pub prefill: Vec<Phase>,
    /// The boot mode of the recovery's first reader: Known (it runs boot-change recovery before it reads after a crash,
    /// [F16] P-60, P-66) or, when `false`, Unknown-boot mode (it reads by the validity rules alone, [OS/proc §5] U2, U5).
    pub reader_known: bool,
    /// A setup-injected state: the newest `HEAD` slot of the prefilled store passes its checksum and fails validity (a
    /// writer defect, [F04 §7] check 5). Every process then exits 7 naming `HEAD` ([F16] P-61), which the harness accepts
    /// as correct, and the operator's remedy is `repair`, which rebuilds both slots from the log ([F16] P-85).
    pub fatal_slot: bool,
    /// A setup-injected extent file beside the prefilled store's own ([`Stray`]); with [`Scenario::fatal_slot`] every
    /// recovery runs `repair`, whose rebuilt state must not take it in.
    pub stray: Option<Stray>,
}

impl Scenario {
    pub fn new(name: &'static str) -> Scenario {
        let mut cfg = Config::test_profile();
        cfg.delete_grace_ms = 0;
        Scenario {
            name,
            cfg,
            store: StoreSetup::Image,
            files: Vec::new(),
            phases: Vec::new(),
            tracked_refs: vec![MAIN],
            rates: FaultRates::default(),
            trace: true,
            init_in_workload: false,
            tick_ns: 1_000,
            prefill: Vec::new(),
            reader_known: true,
            fatal_slot: false,
            stray: None,
        }
    }
}

/// A prefilled store: its files and the ledger calls its operations made.
#[derive(Clone, Debug, Default)]
pub struct Prefilled {
    pub files: Vec<(PathBuf, Vec<u8>)>,
    pub entries: Vec<Entry>,
}

/// The toy with a scenario and its seeded bugs.
#[derive(Clone, Debug)]
pub struct ToySubject {
    pub sc: Arc<Scenario>,
    pub bugs: Bugs,
    /// The prefilled store per boot identity (the prefill is deterministic; the boot identity is the seed's).
    pub prefilled: Arc<Mutex<BTreeMap<[u8; 16], Prefilled>>>,
}

impl ToySubject {
    pub fn new(sc: Scenario, bugs: Bugs) -> ToySubject {
        ToySubject {
            sc: Arc::new(sc),
            bugs,
            prefilled: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// The toy's configuration: the scenario's, with the subject's bugs and the raw facts kept for `doctor --verify`.
    pub fn cfg(&self) -> Config {
        let mut c = self.sc.cfg.clone().with_bugs(self.bugs);
        c.facts = true;
        c
    }
}

/// What the processes of one run share outside the store: the acknowledged groups (for the readers' freshness check and
/// the doctor's chain check).
#[derive(Default)]
pub struct Shared {
    /// Acknowledged operations: their durable groups.
    pub acked: Mutex<Vec<AckedGroup>>,
    /// Acknowledged commit ops.
    pub commits: Mutex<BTreeSet<u64>>,
    /// Acknowledged lazy runtime rows (key, value); every key is written once.
    pub lazy: Mutex<BTreeMap<u64, u64>>,
}

/// The effects a state shows ([`moirai_vfs_sim::enumerate::EffectSet`]).
pub fn effects(st: &State, tracked: &[u64], quiet: bool) -> EffectSet {
    let mut out = EffectSet::new();
    for (&op, c) in &st.commits {
        if (PROBE_OP..PROBE_OP + 16).contains(&op) || c.ref_id == probe_ref(st) {
            continue;
        }
        out.insert(EffectKey::new(EffectKind::Commit, op), c.digest);
    }
    for &name in tracked {
        if let Some(id) = st.ref_id(name)
            && let Some(r) = st.refs.get(&id)
            && r.mover != 0
        {
            out.insert(EffectKey::new(EffectKind::Ref, name), r.mover);
        }
    }
    for (&rid, &op) in &st.orphans {
        if let Some(r) = st.refs.get(&rid) {
            out.insert(EffectKey::new(EffectKind::Other(ORPH), r.name), op);
        }
    }
    for (&uid, m) in &st.markers {
        out.insert(EffectKey::new(EffectKind::Marker, uid), m.op);
    }
    for &uid in st.lease_of.keys() {
        if let Some((_, row)) = st.live_lease(uid) {
            out.insert(EffectKey::new(EffectKind::Lease, uid), row.holder);
        }
    }
    for (&key, row) in &st.idem {
        if row.op != 0 {
            out.insert(EffectKey::new(EffectKind::Idempotency, key), row.result);
        }
    }
    for row in st.intents.values() {
        let v = match row.state {
            moirai_toylog::state::IntentState::Open => OPEN,
            moirai_toylog::state::IntentState::Done { .. } => DONE,
            moirai_toylog::state::IntentState::Aborted { .. } => ABORTED,
        };
        out.insert(EffectKey::new(EffectKind::Intent, row.rec.key), v);
    }
    for &rid in st.pins.keys() {
        if let Some(r) = st.refs.get(&rid)
            && !r.deleted
        {
            out.insert(EffectKey::new(EffectKind::Other(PIN), r.name), 1);
        }
    }
    for (&k, &v) in &st.runtime {
        out.insert(EffectKey::new(EffectKind::Other(RT), k), v);
    }
    if quiet {
        out.insert(EffectKey::new(EffectKind::Other(FLAG), 0), 1);
    }
    out
}

fn probe_ref(st: &State) -> u32 {
    st.ref_id(PROBE_REF).unwrap_or(u32::MAX)
}

fn project_dirs() -> Vec<PathBuf> {
    PROJ.iter().map(PathBuf::from).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Running the acts

/// Where the runner reports what it attempts, acknowledges and observes: the enumerator's ledger, or a record that a
/// prefill replays into the ledger of every run it seeds ([`ToySubject::setup`]).
#[derive(Clone)]
pub enum Sink {
    Ledger(Ledger),
    Record(Arc<Mutex<Vec<Entry>>>),
}

/// One recorded ledger call.
#[derive(Clone, Debug)]
pub enum Entry {
    Begin(u64, Class, Vec<EffectWrite>),
    Ack(u64),
    Observe(EffectKey, Option<u64>),
    Fail(String),
}

impl Sink {
    fn push(&self, e: Entry) {
        match (self, e) {
            (Sink::Ledger(l), Entry::Begin(op, class, writes)) => l.begin(op, class, &writes),
            (Sink::Ledger(l), Entry::Ack(op)) => l.ack(op),
            (Sink::Ledger(l), Entry::Observe(k, v)) => l.observe(k, v),
            (Sink::Ledger(l), Entry::Fail(m)) => l.fail(m),
            (Sink::Record(r), e) => lock(r).push(e),
        }
    }

    pub fn begin(&self, op: u64, class: Class, writes: &[EffectWrite]) {
        self.push(Entry::Begin(op, class, writes.to_vec()));
    }

    pub fn ack(&self, op: u64) {
        self.push(Entry::Ack(op));
    }

    pub fn observe(&self, key: EffectKey, value: Option<u64>) {
        self.push(Entry::Observe(key, value));
    }

    pub fn fail(&self, message: impl Into<String>) {
        self.push(Entry::Fail(message.into()));
    }
}

struct Runner<'a> {
    w: &'a SimWorld,
    l: &'a Sink,
    sub: &'a ToySubject,
    shared: &'a Shared,
    holder: u64,
    claimed: BTreeSet<u64>,
    quiet_byte: Option<u8>,
    /// A lazy effect is acknowledged to the ledger: not before a scripted reboot of the scenario, which may take it
    /// back (a lazy effect survives process deaths, not a system crash, [F15 §4.1]) where the enumerator does not see a
    /// crash of its own.
    lazy_ack: bool,
}

impl Runner<'_> {
    fn ack_group(&self, op: u64, d: &moirai_toylog::Done, durable: bool) {
        if !d.replayed && durable && d.end > 0 {
            lock(&self.shared.acked).push((op, d.start, d.end, d.chain_in, d.chain_out));
        }
    }

    /// After an operation's acknowledgement: the I-G2 check of the process's own view (the view its phase 1 and 2a read)
    /// against the newest slot, then phase 3 ([F16] P-51).
    fn after_ack(&mut self, t: &mut Toy<SimVfs, SimTap>) {
        if let Ok(s) = t.head_for_read()
            && let Some(v) = t.view()
        {
            self.check_view(&v.state, &s);
        }
        t.maintain();
    }

    /// I-G2 over a view a read returned against the published slot ([`uncovered`]).
    fn check_view(&self, st: &State, s: &moirai_toylog::head::Slot) {
        for m in uncovered(self.w, st, s) {
            self.l.fail(m);
        }
    }

    /// An operation that ended with `e`: whether the task goes on, after the check of its exit ([`outcome_unknown`]).
    fn refused(&self, e: &ToyError) -> bool {
        if let Some(m) = outcome_unknown(self.w, e) {
            self.l.fail(m);
        }
        !stops(e)
    }

    fn exec(&mut self, t: &mut Toy<SimVfs, SimTap>, act: &Act) -> bool {
        let l = self.l;
        let tracked = &self.sub.sc.tracked_refs;
        match act {
            Act::Commit {
                op,
                ref_name,
                creates,
                completes,
                key,
                filler,
            } => {
                let mut writes: Vec<EffectWrite> =
                    vec![(EffectKey::new(EffectKind::Commit, *op), Some(dig(*op)))];
                if let Some(uid) = completes {
                    writes.push((EffectKey::new(EffectKind::Marker, *uid), Some(*op)));
                    if self.claimed.contains(uid) {
                        writes.push((EffectKey::new(EffectKind::Lease, *uid), None));
                    }
                }
                if *key {
                    writes.push((EffectKey::new(EffectKind::Idempotency, *op), Some(*op)));
                }
                l.begin(*op, Class::Durable, &writes);
                let track = tracked.contains(ref_name);
                if track {
                    l.begin(
                        *op | REF_BIT,
                        Class::Durable,
                        &[(EffectKey::new(EffectKind::Ref, *ref_name), Some(*op))],
                    );
                }
                let c = CommitOp {
                    op: *op,
                    digest: dig(*op),
                    ref_name: *ref_name,
                    creates: creates.clone(),
                    completes: *completes,
                    holder: self.holder,
                    key: key.then_some(*op),
                    filler: *filler,
                    ..CommitOp::default()
                };
                match t.run(&Op::Commit(c)) {
                    Ok(d) => {
                        l.ack(*op);
                        if track {
                            l.ack(*op | REF_BIT);
                        }
                        if let Some(uid) = completes {
                            self.claimed.remove(uid);
                        }
                        lock(&self.shared.commits).insert(*op);
                        self.ack_group(*op, &d, true);
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Import { op, ref_name } => {
                l.begin(
                    *op,
                    Class::Durable,
                    &[
                        (EffectKey::new(EffectKind::Commit, *op), Some(dig(*op))),
                        (
                            EffectKey::new(EffectKind::Other(ORPH), *ref_name),
                            Some(*op),
                        ),
                        (EffectKey::new(EffectKind::Idempotency, *op), Some(*op)),
                    ],
                );
                let c = CommitOp {
                    op: *op,
                    digest: dig(*op),
                    ref_name: *ref_name,
                    import_old: Some(STALE),
                    key: Some(*op),
                    ..CommitOp::default()
                };
                match t.run(&Op::Commit(c)) {
                    Ok(d) => {
                        l.ack(*op);
                        lock(&self.shared.commits).insert(*op);
                        self.ack_group(*op, &d, true);
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Claim { op, uid } => {
                l.begin(
                    *op,
                    Class::Durable,
                    &[(EffectKey::new(EffectKind::Lease, *uid), Some(self.holder))],
                );
                let c = ClaimOp {
                    op: *op,
                    uid: *uid,
                    holder: self.holder,
                    ttl_ms: self.sub.sc.cfg.lease_ttl_ms,
                    key: Some(*op),
                };
                match t.run(&Op::Claim(c)) {
                    Ok(d) => {
                        l.ack(*op);
                        self.claimed.insert(*uid);
                        self.ack_group(*op, &d, true);
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Release { op, uid } => {
                l.begin(
                    *op,
                    Class::Durable,
                    &[(EffectKey::new(EffectKind::Lease, *uid), None)],
                );
                let r = ReleaseOp {
                    op: *op,
                    uid: *uid,
                    holder: self.holder,
                };
                match t.run(&Op::Release(r)) {
                    Ok(d) => {
                        l.ack(*op);
                        self.claimed.remove(uid);
                        self.ack_group(*op, &d, true);
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Fork { op, from, name } => {
                l.begin(
                    *op,
                    Class::Durable,
                    &[
                        (EffectKey::new(EffectKind::Ref, *name), Some(*op)),
                        (EffectKey::new(EffectKind::Other(PIN), *name), Some(1)),
                    ],
                );
                match t.run(&Op::Fork(ForkOp {
                    op: *op,
                    from: *from,
                    name: *name,
                })) {
                    Ok(d) => {
                        l.ack(*op);
                        self.ack_group(*op, &d, true);
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Runtime {
                op,
                rows,
                pad,
                symbols,
                target,
            } => {
                let writes: Vec<EffectWrite> = rows
                    .iter()
                    .map(|&(k, v)| (EffectKey::new(EffectKind::Other(RT), k), Some(v)))
                    .collect();
                l.begin(*op, Class::Lazy, &writes);
                let r = RuntimeOp {
                    op: *op,
                    rows: rows.clone(),
                    pad: *pad,
                    symbols: symbols.clone(),
                    target_len: *target,
                };
                match t.run(&Op::Runtime(r)) {
                    Ok(_) => {
                        if self.lazy_ack {
                            l.ack(*op);
                        }
                        lock(&self.shared.lazy).extend(rows.iter().copied());
                        self.after_ack(t);
                    }
                    Err(e) => return self.refused(&e),
                }
            }
            Act::Checkpoint => {
                if let Err(e) = t.checkpoint() {
                    return self.refused(&e);
                }
            }
            Act::Mv {
                key,
                file,
                src,
                dst,
            } => {
                return self.file_op(
                    t,
                    FileOp {
                        key: *key,
                        file: *file,
                        src: *src,
                        dst: *dst,
                        op: 1,
                    },
                    Some(at_dir(*dst)),
                );
            }
            Act::Rm {
                key,
                file,
                src,
                trash,
            } => {
                return self.file_op(
                    t,
                    FileOp {
                        key: *key,
                        file: *file,
                        src: *src,
                        dst: 0,
                        op: if *trash { 3 } else { 2 },
                    },
                    trash.then_some(TRASH),
                );
            }
            Act::Bump => {
                if let Err(e) = t.bump_config() {
                    return self.refused(&e);
                }
            }
            Act::Quiet { op, on } => {
                l.begin(
                    *op,
                    Class::Durable,
                    &[(EffectKey::new(EffectKind::Other(FLAG), 0), on.then_some(1))],
                );
                match t.set_quiet(*on) {
                    Ok(_) => l.ack(*op),
                    Err(e) => return self.refused(&e),
                }
            }
            Act::HoldQuiet => {
                if let Ok(k) = t.hold_quiet() {
                    self.quiet_byte = k;
                }
            }
            Act::ReleaseQuiet => {
                if let Some(k) = self.quiet_byte.take() {
                    t.release_quiet(k);
                }
            }
            Act::Read { views } => {
                for _ in 0..*views {
                    if !self.read_once(t) {
                        return false;
                    }
                }
            }
            Act::Doctor => self.doctor(t),
            Act::Adopt => {
                if let Err(e) = t.adopt() {
                    return self.refused(&e);
                }
            }
            Act::RecoverIntents => {
                if let Err(e) = t.recover_intents() {
                    return self.refused(&e);
                }
            }
            Act::Repair => {
                if let Err(e) = t.repair() {
                    return self.refused(&e);
                }
            }
        }
        true
    }

    fn file_op(&mut self, t: &mut Toy<SimVfs, SimTap>, f: FileOp, to: Option<u64>) -> bool {
        let l = self.l;
        let key = f.key;
        let done = key | DONE_TAG;
        l.begin(
            key,
            Class::Durable,
            &[(EffectKey::new(EffectKind::Intent, key), Some(OPEN))],
        );
        // The namespace change takes effect before the commit that closes the intent ([40 §3.4] steps 3–4, [F16] P-17,
        // P-18), so a reader may see it beside the open intent until recovery rolls it forward (P-71): it is also an
        // operation of its own, whose other register is the intent's `done` state — which the intent's own `open` may
        // follow, so the change is never seen without its intent. The closing commit carries the change too: it is
        // appended only after the change is durable (P-17–P-19), so a durable commit never shows without it.
        let ns = (EffectKey::new(EffectKind::Other(NS), f.file), to);
        l.begin(
            key | NS_BIT,
            Class::Durable,
            &[ns, (EffectKey::new(EffectKind::Intent, key), Some(DONE))],
        );
        l.begin(
            done,
            Class::Durable,
            &[
                (EffectKey::new(EffectKind::Commit, done), Some(done)),
                (EffectKey::new(EffectKind::Idempotency, done), Some(done)),
                (EffectKey::new(EffectKind::Intent, key), Some(DONE)),
                ns,
            ],
        );
        l.begin(
            key | ABORT_BIT,
            Class::Durable,
            &[(EffectKey::new(EffectKind::Intent, key), Some(ABORTED))],
        );
        match t.file_op(&f) {
            Ok(_) => {
                l.ack(key);
                l.ack(key | NS_BIT);
                l.ack(done);
                lock(&self.shared.commits).insert(done);
                self.after_ack(t);
                true
            }
            Err(e) => self.refused(&e),
        }
    }

    /// One reader view ([F16 §8]): the read checks of [`ReadWatch`] and [`uncovered`] (a refusal: [`reader_refusal`]),
    /// and every value it shows is observed.
    fn read_once(&mut self, t: &mut Toy<SimVfs, SimTap>) -> bool {
        let before: BTreeSet<u64> = lock(&self.shared.commits).clone();
        let lazy: BTreeMap<u64, u64> = lock(&self.shared.lazy).clone();
        let watch = ReadWatch::begin(self.w, before, lazy);
        let s = match t.refresh() {
            Ok(s) => s,
            Err(e) => {
                if let Some(m) = reader_refusal(self.w, &e, self.sub.sc.fatal_slot) {
                    self.l.fail(m);
                }
                return false;
            }
        };
        let Some(v) = t.view() else {
            return false;
        };
        let st = v.state.clone();
        let whole = v.l0 >= s.committed_lsn;
        self.check_view(&st, &s);
        for m in watch.judge(self.w, &st, whole) {
            self.l.fail(m);
        }
        let quiet = s.flags & moirai_toylog::head::FLAG_QUIET != 0;
        for (k, v) in effects(&st, &self.sub.sc.tracked_refs, quiet) {
            self.l.observe(k, Some(v));
        }
        true
    }

    /// `doctor --verify` inside the scenario: the toy's model checks over the facts and its `HEAD` check, the pinned
    /// files, and the harness's chain check of every acknowledged group ([`chain_breaks`]).
    fn doctor(&mut self, t: &mut Toy<SimVfs, SimTap>) {
        let Ok(st) = t.scratch() else { return };
        for f in verify(&st) {
            self.l.fail(format!("doctor --verify: {f}"));
        }
        // After a failed flush the lazy tail beyond the re-written range may read differently on every read until it
        // is written again ([F15] FM-3.2, FM-3.6), so a reader's replay may show groups the publisher did not fold.
        if self.w.failed_flushes() == 0
            && let Some(p) = t.head_fold_problem()
        {
            self.l.fail(format!("doctor --verify: {p}"));
        }
        if let Ok(missing) = t.missing_pinned_files() {
            for m in missing {
                self.l
                    .fail(format!("doctor --verify: the pinned file {m} is missing"));
            }
        }
        let acked = lock(&self.shared.acked).clone();
        for m in chain_breaks(t.vfs(), self.sub.sc.cfg.extent_bytes, &acked) {
            self.l.fail(m);
        }
    }
}

/// The recovering writer ([`Toy::recover_writer`]): the boot check, the adoption of pending groups, intent recovery
/// where the scenario has project files (no intent exists otherwise), and the probe write — with an operation of its own
/// per attempt, since an attempt that stopped at `store_locked` may have appended its probe.
fn recover_writer(
    t: &mut Toy<SimVfs, SimTap>,
    intents: bool,
    attempt: u64,
) -> Result<(), ToyError> {
    let probe = PROBE_OP + attempt;
    if intents {
        return t.recover_writer(probe);
    }
    t.head_for_read()?;
    t.adopt()?;
    t.run(&Op::Probe(probe)).map(|_| ())
}

/// The fixture of [`Scenario::fatal_slot`]: `head` with its newest valid slot rewritten, checksum included, with
/// `durable_lsn` above `committed_lsn` — a slot that passes its checksum and fails [F04 §7] check 5 (a writer defect).
pub fn fatal_newest_slot(head: &[u8]) -> Vec<u8> {
    use moirai_toylog::head::{SLOT_LEN, Slot, SlotRead};
    let mut out = head.to_vec();
    let newest = (0..2)
        .filter_map(
            |k| match Slot::read(&head[k * SLOT_LEN..(k + 1) * SLOT_LEN]) {
                SlotRead::Valid(s) => Some((k, s)),
                _ => None,
            },
        )
        .max_by_key(|(_, s)| s.slot_seq);
    let Some((k, mut s)) = newest else {
        panic!("the prefilled HEAD has no valid slot");
    };
    s.durable_lsn = s.committed_lsn + 8;
    out[k * SLOT_LEN..(k + 1) * SLOT_LEN].copy_from_slice(&s.to_bytes());
    assert!(matches!(
        Slot::read(&out[k * SLOT_LEN..(k + 1) * SLOT_LEN]),
        SlotRead::Fatal(_)
    ));
    out
}

/// An operator re-running a command that exited 7 `maintenance_busy` or `store_locked` (a dead process's byte is released
/// after FM-8.1's delay; a bounded wait may also time out spuriously, [F15 §3.13]): a second of virtual time apart, at
/// most five times. `f` gets the attempt's number.
fn operator_retry<R>(
    w: &SimWorld,
    mut f: impl FnMut(u64) -> Result<R, ToyError>,
) -> Result<R, ToyError> {
    let mut tries = 0;
    loop {
        match f(tries) {
            Err(ToyError::Busy | ToyError::StoreLocked) if tries < 5 => {
                tries += 1;
                w.advance(1_000_000_000);
            }
            r => return r,
        }
    }
}

/// Errors after which a task stops (the store refuses: corrupt, retired, a refused location, the lock layer).
fn stops(e: &ToyError) -> bool {
    matches!(
        e,
        ToyError::Corrupt(_) | ToyError::Retired | ToyError::Location(_) | ToyError::Lock(_)
    )
}

type Body = Box<dyn FnOnce(SimVfs) + Send>;

impl ToySubject {
    /// Places `init`'s image ([`init::image`]), or nothing for a scenario whose workload runs `init`.
    fn place_image(&self, w: &SimWorld) {
        if self.sc.store == StoreSetup::None {
            w.mkdir_all(Path::new("/sim"));
            return;
        }
        let boot = match self.sc.store {
            StoreSetup::Image => w.boot().1.0,
            _ => [0; 16],
        };
        let cfg = self.cfg();
        let img = image(&cfg, EPOCH, STORE_ID, boot, INIT_WALL_MS);
        w.mkdir_all(&Path::new(STORE).join("tmp"));
        let s = Path::new(STORE);
        w.put_file(&s.join("LOCK"), &img.lock).expect("LOCK");
        w.put_file(&s.join("config"), b"# moirai store configuration\n")
            .expect("config");
        w.put_file(&s.join("log.1"), &self.first_extent(&img.log))
            .expect("log.1");
        w.put_file(&s.join("HEAD"), &img.head).expect("HEAD");
    }

    /// An extent file whose first bytes are `log`: E bytes, zero after `log`.
    fn first_extent(&self, log: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; self.sc.cfg.extent_bytes as usize];
        out[..log.len()].copy_from_slice(log);
        out
    }

    /// The fixture of [`Scenario::stray`] for the prefilled store `pre` of boot `boot`: the extent file's path and bytes.
    /// The path is one the prefilled store does not hold.
    fn stray_extent(&self, stray: Stray, pre: &Prefilled, boot: [u8; 16]) -> (PathBuf, Vec<u8>) {
        let extent = |n: u32| Path::new(STORE).join(format!("log.{n}"));
        let (path, bytes) = match stray {
            Stray::Copy { from, to } => {
                let src = extent(from);
                let Some((_, b)) = pre.files.iter().find(|(p, _)| *p == src) else {
                    panic!("the prefilled store has no {}", src.display());
                };
                (extent(to), b.clone())
            }
            Stray::Foreign => {
                let img = image(&self.cfg(), FOREIGN_EPOCH, STORE_ID, boot, INIT_WALL_MS);
                (extent(1), self.first_extent(&img.log))
            }
        };
        assert!(
            pre.files.iter().all(|(p, _)| *p != path),
            "the prefilled store still holds {}",
            path.display()
        );
        (path, bytes)
    }

    /// Runs the prefill phases in a scratch world whose first boot is `boot`, and returns its store files and the
    /// effects they hold.
    fn prefill(&self, boot: [u8; 16]) -> Prefilled {
        let mut cfg = self.config(PREFILL_SEED);
        cfg.first_boot = Some(moirai_vfs::BootId(boot));
        cfg.trace = moirai_vfs_sim::TraceMode::DigestOnly;
        let sw = SimWorld::new(cfg);
        let record = Arc::new(Mutex::new(Vec::new()));
        self.place_image(&sw);
        self.run_phases(&sw, &Sink::Record(Arc::clone(&record)), &self.sc.prefill);
        let v = sw.process_with("prefill-copy", None, Some(true));
        let mut out = Prefilled::default();
        let root = v
            .open_root(Path::new(STORE), RootRole::Other, RootAccess::Read)
            .expect("the prefilled store");
        for e in v
            .list_dir(&root, None)
            .expect("the prefilled store's entries")
        {
            let Some(name) = e.name.as_segment() else {
                continue;
            };
            if let Found::Bytes(b) = read_all(&v, &root, name) {
                out.files.push((Path::new(STORE).join(name), b));
            }
        }
        out.entries = lock(&record).clone();
        out
    }

    /// Runs `phases`: each phase's driver actions, then its processes to their end.
    fn run_phases(&self, w: &SimWorld, l: &Sink, phases: &[Phase]) {
        let shared = Arc::new(Shared::default());
        for (pi, phase) in phases.iter().enumerate() {
            for d in &phase.before {
                match d {
                    DriverAct::Advance(ns) => w.advance(*ns),
                    DriverAct::Crash => {
                        if w.crash(&CrashPlan::baseline()).is_err() {
                            return;
                        }
                        lock(&shared.lazy).clear();
                    }
                }
            }
            let mut tasks = Vec::new();
            for (qi, p) in phase.procs.iter().enumerate() {
                let v = w.process_with(&p.name, None, p.known);
                if p.wall_step_ms != 0 {
                    w.step_wall(&v, p.wall_step_ms);
                }
                let locks = ProcLocks::new();
                let holder = (pi as u64) << 16 | (qi as u64 + 1);
                for (ti, acts) in p.tasks.iter().enumerate() {
                    let (sub, lg, wd, sh, lk) = (
                        self.clone(),
                        l.clone(),
                        w.clone(),
                        Arc::clone(&shared),
                        locks.clone(),
                    );
                    let acts = acts.clone();
                    let init_here = self.sc.init_in_workload && pi == 0 && qi == 0 && ti == 0;
                    let lazy_ack = !phases[pi + 1..]
                        .iter()
                        .any(|p| p.before.iter().any(|d| matches!(d, DriverAct::Crash)));
                    let body: Body = Box::new(move |v: SimVfs| {
                        wd.note(T_TASK, u64::from(v.self_id().pid), ti as u64);
                        if init_here
                            && moirai_toylog::init(&v, Path::new(STORE), &sub.cfg()).is_err()
                        {
                            return;
                        }
                        let tap = SimTap::new(&wd, &v);
                        let Ok(mut t) = Toy::open(v, Path::new(STORE), sub.cfg(), tap, lk) else {
                            return;
                        };
                        t.set_project_dirs(&project_dirs());
                        let mut r = Runner {
                            w: &wd,
                            l: &lg,
                            sub: &sub,
                            shared: &sh,
                            holder,
                            claimed: BTreeSet::new(),
                            quiet_byte: None,
                            lazy_ack,
                        };
                        for a in &acts {
                            if !r.exec(&mut t, a) {
                                break;
                            }
                        }
                        if let Some(k) = r.quiet_byte.take() {
                            t.release_quiet(k);
                        }
                    });
                    tasks.push(w.spawn(&v, body));
                }
            }
            let _ = w.run();
            for t in tasks {
                if let Some(TaskEnd::Panicked(m)) = t.end() {
                    panic!("a task panicked: {m}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The subject

impl Subject for ToySubject {
    fn config(&self, seed: u64) -> SimConfig {
        let mut c = SimConfig::new(seed);
        c.rates = self.sc.rates;
        c.tick_ns = self.sc.tick_ns;
        c.volumes = vec![(PathBuf::from(VOL2), VolumeProfile::default())];
        c
    }

    fn setup(&self, w: &SimWorld, l: &Ledger) {
        w.mkdir_all(Path::new("/sim/proj/a"));
        w.mkdir_all(Path::new("/sim/proj/b"));
        w.mkdir_all(Path::new("/sim/vol2/c"));
        for (i, &(f, dir)) in self.sc.files.iter().enumerate() {
            let p = Path::new(PROJ[usize::from(dir) - 1]).join(file_name(f));
            w.put_file(&p, &content(f)).expect("a project file");
            let op = 0x5E7_0000 + i as u64;
            let writes = [(EffectKey::new(EffectKind::Other(NS), f), Some(at_dir(dir)))];
            l.begin(op, Class::Durable, &writes);
            l.ack(op);
        }
        if self.sc.store == StoreSetup::None {
            w.mkdir_all(Path::new("/sim"));
            return;
        }
        if self.sc.prefill.is_empty() {
            self.place_image(w);
            return;
        }
        let boot = w.boot().1.0;
        let pre = {
            let mut cache = lock(&self.prefilled);
            cache
                .entry(boot)
                .or_insert_with(|| self.prefill(boot))
                .clone()
        };
        w.mkdir_all(&Path::new(STORE).join("tmp"));
        for (path, bytes) in &pre.files {
            if self.sc.fatal_slot && path.ends_with("HEAD") {
                w.put_file(path, &fatal_newest_slot(bytes))
                    .expect("the fixture's HEAD");
            } else {
                w.put_file(path, bytes).expect("a prefilled store file");
            }
        }
        if let Some(stray) = self.sc.stray {
            let (path, bytes) = self.stray_extent(stray, &pre, boot);
            w.put_file(&path, &bytes).expect("the fixture's extent");
        }
        let sink = Sink::Ledger(l.clone());
        for e in pre.entries {
            sink.push(e);
        }
    }

    fn workload(&self, w: &SimWorld, l: &Ledger) {
        self.run_phases(w, &Sink::Ledger(l.clone()), &self.sc.phases);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let tracked = &self.sc.tracked_refs;
        let files: Vec<u64> = self.sc.files.iter().map(|&(f, _)| f).collect();
        let cfg = self.cfg();
        let ns = |v: &SimVfs, set: &mut EffectSet| {
            for &f in &files {
                if let Some(loc) = ns_location(v, f) {
                    set.insert(EffectKey::new(EffectKind::Other(NS), f), loc);
                }
            }
        };
        // Before `init` named `HEAD` there is no store: nothing to read, nothing to recover.
        if !w.exists(&Path::new(STORE).join("HEAD")) {
            let mut set = EffectSet::new();
            let v = w.process_with("recovery-reader", None, Some(true));
            ns(&v, &mut set);
            return Recovered::same(set);
        }
        let damaged_head = head_may_be_damaged(w);
        // The refusals the operator answers with `repair` ([F16] P-85): no valid slot where the fault model may have
        // damaged both (see `head_may_be_damaged`), and the fixture's fatal slot ([F16] P-61: exit 7 naming `HEAD`).
        let heal = |m: &str| {
            (m.contains("no valid slot") && damaged_head)
                || (self.sc.fatal_slot && m.starts_with("HEAD slot"))
        };
        // The first read: a reader's first read (with the boot-change recovery P-60 makes a Known-boot reader run first).
        let rv = w.process_with("recovery-reader", None, Some(self.sc.reader_known));
        let first_read = (|| -> Result<EffectSet, ToyError> {
            let tap = SimTap::new(w, &rv);
            let mut t = Toy::open(
                rv.clone(),
                Path::new(STORE),
                cfg.clone(),
                tap,
                ProcLocks::new(),
            )?;
            // A store with no valid HEAD slot exits 7 naming `moirai repair` ([F16] P-61): where a slot may be damaged
            // (see `head_may_be_damaged`) that is the expected outcome, and the operator runs `repair`, then reads;
            // otherwise it is a refusal. The fixture's fatal slot is answered the same way.
            let s = match operator_retry(w, |_| t.refresh()) {
                Err(ToyError::Corrupt(m)) if heal(&m) => {
                    operator_retry(w, |_| t.repair_head())?;
                    operator_retry(w, |_| t.refresh())?
                }
                r => r?,
            };
            let st = t.view().map(|v| v.state.clone()).unwrap_or_default();
            let mut set = effects(&st, tracked, s.flags & moirai_toylog::head::FLAG_QUIET != 0);
            ns(&rv, &mut set);
            Ok(set)
        })()
        .map_err(|e| format!("{e}"));
        // The recovering writer: repair of a damaged sealed file, adoption, intent recovery, a probe write.
        let wv = w.process_with("recovery-writer", None, Some(true));
        let mut findings = Vec::new();
        let mut diagnosed = Vec::new();
        let state = (|| -> Result<EffectSet, ToyError> {
            let tap = SimTap::new(w, &wv);
            let mut t = Toy::open(
                wv.clone(),
                Path::new(STORE),
                cfg.clone(),
                tap,
                ProcLocks::new(),
            )?;
            t.set_project_dirs(&project_dirs());
            let damaged = match operator_retry(w, |_| t.repair()) {
                Err(ToyError::Corrupt(m)) if heal(&m) => {
                    operator_retry(w, |_| t.repair_head())?;
                    operator_retry(w, |_| t.repair())?
                }
                r => r?,
            };
            diagnosed.extend(damaged.iter().map(|n| Path::new(STORE).join(n)));
            // A HEAD whose slots a failed flush poisoned reads differently each time (FM-3.2): a read that finds no
            // valid slot sends the operator to `repair` (OP-1), then the recovery runs again.
            let intents = !self.sc.files.is_empty();
            match operator_retry(w, |k| recover_writer(&mut t, intents, k)) {
                Err(ToyError::Corrupt(m)) if heal(&m) => {
                    operator_retry(w, |_| t.repair_head())?;
                    operator_retry(w, |k| recover_writer(&mut t, intents, 8 + k))?;
                }
                r => r?,
            }
            let s = t.refresh()?;
            let st = t.view().map(|v| v.state.clone()).unwrap_or_default();
            findings.extend(
                verify(&st)
                    .into_iter()
                    .map(|f| format!("doctor --verify: {f}")),
            );
            // As in the scenario's `doctor`: not after a failed flush (FM-3.2, FM-3.6).
            if w.failed_flushes() == 0
                && let Some(p) = t.head_fold_problem()
            {
                findings.push(format!("doctor --verify: {p}"));
            }
            if !st.pins.is_empty() {
                for m in t.missing_pinned_files()? {
                    findings.push(format!("doctor --verify: the pinned file {m} is missing"));
                }
            }
            findings.extend(intent_namespace(&wv, &st));
            let mut set = effects(&st, tracked, s.flags & moirai_toylog::head::FLAG_QUIET != 0);
            ns(&wv, &mut set);
            Ok(set)
        })()
        .map_err(|e| format!("{e}"));
        Recovered {
            first_read,
            state,
            findings,
            diagnosed,
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        vec![Path::new(STORE).join("HEAD")]
    }

    fn checks_trace(&self) -> bool {
        self.sc.trace
    }

    fn check_trace(&self, events: &[Event]) -> Vec<String> {
        trace_violations(events)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The scenarios

/// An operation id of process tag `p`, step `i`.
pub const fn op(p: u64, i: u64) -> u64 {
    p << 32 | i
}

/// A uid of process tag `p`.
pub const fn uid(p: u64, i: u64) -> u64 {
    p << 32 | (0x1000 + i)
}

/// A commit on `main` creating one node, with its idempotency key.
pub fn commit(p: u64, i: u64, filler: u32) -> Act {
    Act::Commit {
        op: op(p, i),
        ref_name: MAIN,
        creates: vec![uid(p, i)],
        completes: None,
        key: true,
        filler,
    }
}

/// A phase with no driver action.
pub fn phase(procs: Vec<ProcPlan>) -> Phase {
    Phase {
        before: Vec::new(),
        procs,
    }
}

/// The final `doctor --verify` phase.
pub fn doctor() -> Phase {
    phase(vec![ProcPlan::new("doctor", vec![Act::Doctor])])
}

/// A small lazy runtime batch.
pub fn rt(p: u64, i: u64, key: u64, pad: u32) -> Act {
    Act::Runtime {
        op: op(p, i),
        rows: vec![(key, op(p, i))],
        pad,
        symbols: Vec::new(),
        target: 0,
    }
}

/// Group commit with three writers, a reader and a lazy appender (P-1, P-2, P-25, P-28-P-50, P-57, P-58).
pub fn basic() -> Scenario {
    let mut sc = Scenario::new("basic");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 100), commit(1, 2, 700)]),
            ProcPlan::new("w2", vec![commit(2, 1, 300)]),
            ProcPlan::new("w3", vec![commit(3, 1, 200)]),
            ProcPlan::new("r", vec![Act::Read { views: 2 }]),
            ProcPlan::new("lazy", vec![rt(4, 1, 0xE1, 64)]),
        ]),
        doctor(),
    ];
    sc
}

/// Leases: claims of one task that conflict, a completion with its marker, a claim and its release (P-5, P-31, P-34,
/// P-52, P-65).
pub fn leases() -> Scenario {
    let t = uid(9, 1);
    let mut sc = Scenario::new("leases");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new(
                "w1",
                vec![
                    Act::Claim {
                        op: op(1, 1),
                        uid: t,
                    },
                    Act::Commit {
                        op: op(1, 2),
                        ref_name: MAIN,
                        creates: vec![t],
                        completes: Some(t),
                        key: true,
                        filler: 100,
                    },
                ],
            ),
            ProcPlan::new(
                "w2",
                vec![
                    Act::Claim {
                        op: op(2, 1),
                        uid: t,
                    },
                    Act::Commit {
                        op: op(2, 2),
                        ref_name: MAIN,
                        creates: vec![uid(2, 9)],
                        completes: None,
                        key: true,
                        filler: 100,
                    },
                ],
            ),
            ProcPlan::new(
                "w3",
                vec![
                    Act::Claim {
                        op: op(3, 1),
                        uid: uid(3, 1),
                    },
                    Act::Release {
                        op: op(3, 2),
                        uid: uid(3, 1),
                    },
                ],
            ),
        ]),
        doctor(),
    ];
    sc
}

/// A lazy batch of about `kib` KiB.
pub fn big(p: u64, i: u64, kib: u32) -> Act {
    rt(p, i, op(p, i), kib * 1024)
}

/// Rotation into a new extent: the store starts with most of log.1 filled (a prefilled lazy batch made durable by a
/// commit), then two writers and a lazy appender whose groups no longer fit it; the first of them prepares log.2 under
/// the flush byte and appends its extent head, and the flushed range spans both extents (P-1, P-6, P-8, P-9, P-72,
/// P-97).
pub fn rotate() -> Scenario {
    let mut sc = Scenario::new("rotate");
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "l",
        vec![big(1, 1, 50), commit(1, 2, 100)],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(2, 1, 3000)]),
            ProcPlan::new("w2", vec![commit(3, 1, 3000)]),
            ProcPlan::new("l2", vec![big(4, 1, 12)]),
        ]),
        doctor(),
    ];
    sc
}

/// A spare extent: maintenance finds the log past E / 2 and prepares log.2 under a temporary name beside a writer whose
/// last commit no longer fits log.1; the rotation re-issues the spare's flushes before its extent head enters it, which
/// matters when the writer's handle made the store directory's names durable before the spare was renamed (P-96, P-72
/// step 2).
pub fn spare() -> Scenario {
    let mut sc = Scenario::new("spare");
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "l",
        vec![big(1, 1, 60), commit(1, 2, 100)],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m", vec![Act::Checkpoint]),
            ProcPlan::new(
                "w",
                vec![commit(2, 1, 100), commit(2, 2, 100), commit(2, 4, 3000)],
            ),
        ]),
        doctor(),
    ];
    sc
}

/// Retirement with one active extent beside readers that pauses may catch in the middle of a replay of the retired
/// extent (FM-6): the extent's file is deleted after the barrier, never reused (P-74).
pub fn reuse() -> Scenario {
    let mut sc = Scenario::new("reuse");
    sc.cfg.active_extents = 1;
    sc.rates.pause = 50_000;
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "l",
        vec![
            big(1, 1, 50),
            commit(1, 2, 3000),
            big(1, 3, 12),
            commit(1, 4, 100),
        ],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m", vec![Act::Checkpoint]),
            ProcPlan::new("r1", vec![Act::Read { views: 1 }]),
            ProcPlan::new("r2", vec![Act::Read { views: 1 }]),
        ]),
        doctor(),
    ];
    sc
}

/// Retirement with one active extent: the store starts rotated into log.2 (prefilled), then a checkpoint retires log.1
/// beside a lazy appender that rotates again and a reader that replays from the old set; the barrier comes before the
/// deletion (P-73, P-74, P-77 for extents, P-59).
pub fn retire() -> Scenario {
    let mut sc = Scenario::new("retire");
    sc.cfg.active_extents = 1;
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "l",
        vec![
            big(1, 1, 50),
            commit(1, 2, 3000),
            big(1, 3, 12),
            commit(1, 4, 100),
        ],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m", vec![Act::Checkpoint]),
            ProcPlan::new("l2", vec![big(2, 1, 40)]),
            ProcPlan::new("r", vec![Act::Read { views: 1 }]),
        ]),
        doctor(),
    ];
    sc
}

/// The prefill of a store whose first extent is retired and deleted (one active extent): log.1 filled and rotated out of
/// into log.2, then a checkpoint that retires log.1 into its `hist` file and deletes it after the barrier (P-73, P-14).
pub fn first_extent_retired() -> Vec<Phase> {
    vec![
        phase(vec![ProcPlan::new(
            "l",
            vec![
                big(1, 1, 50),
                commit(1, 2, 3000),
                big(1, 3, 12),
                commit(1, 4, 100),
            ],
        )]),
        phase(vec![ProcPlan::new("m", vec![Act::Checkpoint])]),
    ]
}

/// After a retirement and its deletion (prefilled): a writer's publishes beside a reader that starts from `HEAD`, whose
/// other slot must never name the deleted extent (P-12).
pub fn stale() -> Scenario {
    let mut sc = Scenario::new("stale");
    sc.cfg.active_extents = 1;
    sc.prefill = first_extent_retired();
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w", vec![commit(3, 1, 100)]),
            ProcPlan::new("r", vec![Act::Read { views: 1 }]),
        ]),
        doctor(),
    ];
    sc
}

/// A checkpoint that decides while the log is still in log.1 and retires beside an appender that rotates into log.2
/// (one active extent): the retirement bound EX-5 (P-73).
pub fn boundary() -> Scenario {
    let mut sc = Scenario::new("boundary");
    sc.cfg.active_extents = 1;
    // Pauses (FM-6) let the rotation publish between the checkpoint's decision and its retirement.
    sc.rates.pause = 50_000;
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "l",
        vec![big(1, 1, 50), commit(1, 2, 100)],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m", vec![Act::Checkpoint]),
            ProcPlan::new("l2", vec![big(2, 1, 20)]),
        ]),
        doctor(),
    ];
    sc
}

/// A checkpoint between commits of one millisecond: its segment, its `Checkpoint` group, the barrier after it (P-10,
/// P-13, P-36, P-50, P-62).
pub fn ckpt() -> Scenario {
    let mut sc = Scenario::new("ckpt");
    // Everything within a few milliseconds: HLCs come from the sequence, not the wall clock (P-36).
    sc.tick_ns = 10;
    sc.phases = vec![
        phase(vec![ProcPlan::new("w", vec![commit(1, 1, 300)])]),
        phase(vec![
            ProcPlan::new("m", vec![Act::Checkpoint, commit(2, 1, 100)]),
            ProcPlan::new("w2", vec![commit(3, 1, 200)]),
        ]),
        doctor(),
    ];
    sc
}

/// A second checkpoint releases the first one's segment and deletes it after its barrier, beside a reader that loaded
/// the old set (P-12, P-14, P-59, P-77).
pub fn gc() -> Scenario {
    let mut sc = Scenario::new("gc");
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "m",
        vec![commit(1, 1, 300), Act::Checkpoint],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m2", vec![commit(2, 1, 300), Act::Checkpoint]),
            ProcPlan::new("r", vec![Act::Read { views: 2 }]),
        ]),
        doctor(),
    ];
    sc
}

/// A checkpoint alone after a prefilled history, read after a crash by an Unknown-boot reader, which runs no boot-change
/// recovery ([OS/proc §5] U2) and so reads the slots exactly as the crash left them: no operation of the workload is
/// acknowledged, so every state the barrier and the deletion leave must still serve the prefilled history (P-13, P-14,
/// P-62).
pub fn barrier() -> Scenario {
    let mut sc = Scenario::new("barrier");
    sc.reader_known = false;
    // The prefill ends with two durable publishes (`quiet on`, `quiet off`), so both slots hold its last state durably:
    // a crash that reverts the workload's unflushed publishes leaves a slot with every prefilled commit.
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "m",
        vec![
            commit(1, 1, 300),
            Act::Checkpoint,
            commit(1, 2, 300),
            Act::Quiet {
                op: op(1, 3),
                on: true,
            },
            Act::Quiet {
                op: op(1, 4),
                on: false,
            },
        ],
    )])];
    sc.phases = vec![phase(vec![ProcPlan::new("m2", vec![Act::Checkpoint])])];
    sc
}

/// A prefilled store whose newest `HEAD` slot passes its checksum and fails validity ([F04 §7], a setup-injected writer
/// defect): every process must refuse it (exit 7, [F16] P-61) and never fall back to the older slot, until `repair`
/// rebuilds both slots from the log. The recovery's first reader is in Unknown-boot mode, so no boot-change recovery
/// rewrites the slots before it reads.
pub fn fatal() -> Scenario {
    let mut sc = Scenario::new("fatal");
    sc.reader_known = false;
    sc.fatal_slot = true;
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "w",
        vec![commit(1, 1, 200), commit(1, 2, 200)],
    )])];
    sc.phases = vec![phase(vec![
        ProcPlan::new("w2", vec![commit(2, 1, 200)]),
        ProcPlan::new("r", vec![Act::Read { views: 1 }]),
    ])];
    sc
}

/// A store without a valid `HEAD` slot to repair from a log whose lowest extent number is taken by a stray file:
/// [`first_extent_retired`]'s store, the fixture's fatal slot (so every recovery runs `repair`, [F16] P-85) and
/// `stray`. The workload's processes refuse the store (P-61); the recovery must rebuild the prefilled state from log.2.
fn stray_repair(name: &'static str, stray: Stray) -> Scenario {
    let mut sc = Scenario::new(name);
    sc.cfg.active_extents = 1;
    sc.reader_known = false;
    sc.fatal_slot = true;
    sc.stray = Some(stray);
    sc.prefill = first_extent_retired();
    sc.phases = vec![phase(vec![
        ProcPlan::new("w", vec![commit(3, 1, 100)]),
        ProcPlan::new("r", vec![Act::Read { views: 1 }]),
    ])];
    sc
}

/// [`stray_repair`] with a byte copy of log.2 as log.1: `repair`'s step 2 must refuse the copy's head by its position
/// (P-54, T13); a repair that accepts it scans the copy from lsn 0 by the chain value its head carries.
pub fn misplaced() -> Scenario {
    stray_repair("misplaced", Stray::Copy { from: 2, to: 1 })
}

/// [`stray_repair`] with log.1 of another epoch: `repair`'s step 2 must refuse its head by its epoch (P-55); a repair
/// that accepts it scans that extent from lsn 0 by the chain value its head carries.
pub fn foreign() -> Scenario {
    stray_repair("foreign", Stray::Foreign)
}

/// `quiet on` and `quiet off` (durable publishes) in the prefilled history, then two commits whose publishes write both
/// slots again without a `HEAD` flush: a crash may tear the newer slot and revert the older one to its durable content,
/// which must never bring back a flag state that an acknowledged flag change replaced (P-13). The flag changes are in the
/// prefill, so no flag change of the workload is left unacknowledged in poisoned `HEAD` sectors, whose reads may differ
/// ([F15] FM-3.2).
pub fn flags() -> Scenario {
    let mut sc = Scenario::new("flags");
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "q",
        vec![
            commit(1, 1, 100),
            Act::Quiet {
                op: op(1, 2),
                on: true,
            },
            Act::Quiet {
                op: op(1, 3),
                on: false,
            },
        ],
    )])];
    sc.phases = vec![phase(vec![ProcPlan::new(
        "w",
        vec![commit(2, 1, 100), commit(2, 2, 100)],
    )])];
    sc
}

/// Forks pin the checkpoint set they start from. The store starts with two checkpoints and a fork between them, whose
/// pin keeps the first set's segment although the second released it; then a third checkpoint releases the second set
/// beside a fork of it whose group the deciding maintenance has not seen published (P-77, P-79, P-81).
pub fn fork() -> Scenario {
    let mut sc = Scenario::new("fork");
    sc.tracked_refs = vec![MAIN, 0xF1, 0xF2];
    sc.prefill = vec![
        phase(vec![ProcPlan::new(
            "m",
            vec![commit(1, 1, 300), Act::Checkpoint],
        )]),
        phase(vec![ProcPlan::new(
            "f",
            vec![Act::Fork {
                op: op(2, 1),
                from: MAIN,
                name: 0xF1,
            }],
        )]),
        phase(vec![ProcPlan::new(
            "m2",
            vec![commit(3, 1, 100), Act::Checkpoint],
        )]),
    ];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("m3", vec![commit(5, 1, 100), Act::Checkpoint]),
            ProcPlan::new(
                "f2",
                vec![Act::Fork {
                    op: op(4, 1),
                    from: MAIN,
                    name: 0xF2,
                }],
            ),
        ]),
        doctor(),
    ];
    sc
}

/// `file mv` and `file rm` beside each other, then intent recovery (P-16, P-17, P-18, P-19).
pub fn intents() -> Scenario {
    let mut sc = Scenario::new("intents");
    sc.tracked_refs = Vec::new();
    sc.files = vec![(1, 1), (2, 1)];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new(
                "c1",
                vec![Act::Mv {
                    key: op(1, 1),
                    file: 1,
                    src: 1,
                    dst: 2,
                }],
            ),
            ProcPlan::new(
                "c2",
                vec![Act::Rm {
                    key: op(2, 1),
                    file: 2,
                    src: 1,
                    trash: false,
                }],
            ),
        ]),
        Phase {
            before: vec![DriverAct::Advance(50_000_000)],
            procs: vec![ProcPlan::new("rec", vec![Act::RecoverIntents, Act::Doctor])],
        },
    ];
    sc
}

/// `file rm --trash`: the rename into `trash/<intent>/0` with its directory barriers, then intent recovery (P-18,
/// P-82).
pub fn trash() -> Scenario {
    let mut sc = Scenario::new("trash");
    sc.tracked_refs = Vec::new();
    sc.files = vec![(3, 1)];
    sc.phases = vec![
        phase(vec![ProcPlan::new(
            "c",
            vec![Act::Rm {
                key: op(3, 1),
                file: 3,
                src: 1,
                trash: true,
            }],
        )]),
        Phase {
            before: vec![DriverAct::Advance(50_000_000)],
            procs: vec![ProcPlan::new("rec", vec![Act::RecoverIntents, Act::Doctor])],
        },
    ];
    sc
}

/// A `file mv` across volumes, which is refused before its intent (P-83).
pub fn xvol() -> Scenario {
    let mut sc = Scenario::new("xvol");
    sc.tracked_refs = Vec::new();
    sc.files = vec![(4, 2)];
    sc.phases = vec![phase(vec![ProcPlan::new(
        "c",
        vec![Act::Mv {
            key: op(4, 1),
            file: 4,
            src: 2,
            dst: 3,
        }],
    )])];
    sc
}

/// A store made by an `init` in Unknown-boot mode (a zero `boot_id`), Unknown-boot writers, and a Known-boot process
/// whose first read runs boot-change recovery (P-15, P-60, P-66, P-67).
pub fn boot() -> Scenario {
    let mut sc = Scenario::new("boot");
    sc.store = StoreSetup::ImageUnknownBoot;
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("u1", vec![commit(1, 1, 200)]).unknown_boot(),
            ProcPlan::new("u2", vec![commit(2, 1, 300)]).unknown_boot(),
            ProcPlan::new("k", vec![Act::Read { views: 1 }, commit(3, 1, 100)]),
        ]),
        doctor(),
    ];
    sc
}

/// A scripted reboot inside the scenario: commits acknowledged before it whose publish the crash lost, then an
/// Unknown-boot writer (P-29, P-64, P-92).
pub fn reboot() -> Scenario {
    let mut sc = Scenario::new("reboot");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 300), commit(1, 2, 300)]),
            ProcPlan::new("w2", vec![commit(2, 1, 300)]),
        ]),
        Phase {
            before: vec![DriverAct::Crash],
            procs: vec![ProcPlan::new("u", vec![commit(3, 1, 200)]).unknown_boot()],
        },
        doctor(),
    ];
    sc
}

/// `init` inside the scenario, then commits and a fork (P-24, P-88).
pub fn init_store() -> Scenario {
    let mut sc = Scenario::new("init");
    sc.store = StoreSetup::None;
    sc.init_in_workload = true;
    sc.tracked_refs = vec![MAIN, 0xF1];
    sc.phases = vec![
        phase(vec![ProcPlan::new("i", vec![commit(1, 1, 100)])]),
        phase(vec![ProcPlan::new(
            "w",
            vec![
                Act::Fork {
                    op: op(2, 1),
                    from: MAIN,
                    name: 0xF1,
                },
                commit(2, 2, 100),
                Act::Doctor,
            ],
        )]),
    ];
    sc
}

/// One process with three clients (tasks) and another process (P-3, L-6), with pauses (FM-6) that may stop a client
/// between its scan and its append while a sibling appends, flushes and acknowledges.
pub fn inproc() -> Scenario {
    let mut sc = Scenario::new("inproc");
    sc.rates.pause = 50_000;
    sc.phases = vec![
        phase(vec![
            ProcPlan::with_tasks(
                "p",
                vec![
                    vec![commit(1, 1, 300)],
                    vec![commit(2, 1, 300)],
                    vec![commit(3, 1, 300)],
                ],
            ),
            ProcPlan::new("q", vec![commit(4, 1, 300)]),
        ]),
        doctor(),
    ];
    sc
}

/// `config_gen` bumps and `quiet` flags beside commits (P-4, P-48, P-63).
pub fn admin() -> Scenario {
    let mut sc = Scenario::new("admin");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 200)]),
            ProcPlan::new(
                "a",
                vec![
                    Act::Bump,
                    Act::Quiet {
                        op: op(2, 1),
                        on: true,
                    },
                    Act::Quiet {
                        op: op(2, 2),
                        on: false,
                    },
                ],
            ),
            ProcPlan::new("w2", vec![commit(3, 1, 200)]),
        ]),
        doctor(),
    ];
    sc
}

/// Imports whose ref CAS fails, and the parks the next durable appenders write (P-69, P-70).
pub fn import() -> Scenario {
    let mut sc = Scenario::new("import");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 200)]),
            ProcPlan::new(
                "i",
                vec![
                    Act::Import {
                        op: op(2, 1),
                        ref_name: MAIN,
                    },
                    Act::Import {
                        op: op(2, 2),
                        ref_name: MAIN,
                    },
                ],
            ),
            ProcPlan::new("w2", vec![commit(3, 1, 200)]),
        ]),
        doctor(),
    ];
    sc
}

/// A long-lived process that keeps its view between writes and reads, beside lazy appenders and a writer (P-30, P-39,
/// P-49, P-56).
pub fn server() -> Scenario {
    let mut sc = Scenario::new("server");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new(
                "srv",
                vec![
                    rt(1, 1, 0xE1, 200),
                    commit(1, 2, 200),
                    Act::Read { views: 1 },
                    rt(1, 3, 0xE4, 200),
                    Act::Read { views: 1 },
                ],
            ),
            ProcPlan::new("l", vec![rt(2, 1, 0xE2, 200), rt(2, 2, 0xE3, 200)]),
            ProcPlan::new("w", vec![commit(3, 1, 300)]),
        ]),
        doctor(),
    ];
    sc
}

/// Short lock waits with spurious timeouts ([F15 §3.13]) and pauses (FM-6) that outlast them (P-27, P-41, L-7).
pub fn timeouts() -> Scenario {
    let mut sc = Scenario::new("timeouts");
    sc.cfg.writer_wait_ms = 100;
    sc.cfg.flush_wait_ms = 100;
    sc.rates.spurious_wake = 300_000;
    sc.rates.pause = 20_000;
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 200), commit(1, 2, 200)]),
            ProcPlan::new("w2", vec![commit(2, 1, 200)]),
            ProcPlan::new("w3", vec![commit(3, 1, 200)]),
        ]),
        doctor(),
    ];
    sc
}

/// One operation submitted by three processes with one idempotency key, beside another writer (P-32, P-33).
pub fn retry() -> Scenario {
    let mut sc = Scenario::new("retry");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("a", vec![commit(1, 1, 300), commit(9, 1, 400)]),
            ProcPlan::new("b", vec![commit(9, 1, 400)]),
            ProcPlan::new("c", vec![commit(9, 1, 400)]),
        ]),
        doctor(),
    ];
    sc
}

/// Intent recovery running beside live movers, with probes that may answer `Unknown` ([F15 §3.13]) (P-71).
pub fn intents_live() -> Scenario {
    let mut sc = Scenario::new("intents_live");
    sc.tracked_refs = Vec::new();
    sc.files = vec![(1, 1), (2, 1)];
    sc.rates.probe_unknown = 500_000;
    sc.phases = vec![phase(vec![
        ProcPlan::new(
            "c1",
            vec![Act::Mv {
                key: op(1, 1),
                file: 1,
                src: 1,
                dst: 2,
            }],
        ),
        ProcPlan::new(
            "c2",
            vec![Act::Rm {
                key: op(2, 1),
                file: 2,
                src: 1,
                trash: false,
            }],
        ),
        ProcPlan::new(
            "rec",
            vec![
                Act::RecoverIntents,
                Act::RecoverIntents,
                Act::RecoverIntents,
            ],
        ),
    ])];
    sc
}

/// A claim beside a process whose wall clock stepped forward by an hour (FM-7.1) (P-89).
pub fn clock() -> Scenario {
    let t = uid(9, 1);
    let mut sc = Scenario::new("clock");
    let mut late = ProcPlan::new(
        "w2",
        vec![
            Act::Claim {
                op: op(2, 1),
                uid: t,
            },
            Act::Claim {
                op: op(2, 2),
                uid: t,
            },
        ],
    );
    late.wall_step_ms = 3_600_000;
    sc.phases = vec![
        phase(vec![ProcPlan::new(
            "w1",
            vec![
                Act::Claim {
                    op: op(1, 1),
                    uid: t,
                },
                commit(1, 2, 100),
            ],
        )]),
        phase(vec![late]),
        doctor(),
    ];
    sc
}

/// Two quiet requesters whose bytes overlap, and maintenance deciding beside them ([F03 §3.1]) (L-8).
pub fn quiet() -> Scenario {
    let mut sc = Scenario::new("quiet");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new(
                "q1",
                vec![Act::HoldQuiet, commit(2, 1, 100), Act::ReleaseQuiet],
            ),
            ProcPlan::new(
                "q2",
                vec![
                    Act::HoldQuiet,
                    commit(3, 1, 100),
                    commit(3, 2, 100),
                    Act::ReleaseQuiet,
                ],
            ),
            ProcPlan::new(
                "m",
                vec![commit(4, 1, 100), Act::Checkpoint, Act::Checkpoint],
            ),
        ]),
        doctor(),
    ];
    sc
}

/// Runtime batches sized to the largest group W3 allows by their phase-1 encoding, whose final encoding grows under
/// the writer byte by a symbol id that another writer's pending group took (P-35).
pub fn sizes() -> Scenario {
    let mut sc = Scenario::new("sizes");
    let limit = sc.cfg.extent_bytes - moirai_toylog::format::ROTATION_RESERVE;
    let setup: Vec<String> = (0..126).map(|i| format!("sym{i}")).collect();
    let big = |p: u64, s: &str| Act::Runtime {
        op: op(p, 1),
        rows: vec![(op(p, 1), 1)],
        pad: 0,
        symbols: vec![s.to_owned()],
        target: limit,
    };
    sc.prefill = vec![phase(vec![ProcPlan::new(
        "s",
        vec![
            Act::Runtime {
                op: op(1, 1),
                rows: vec![(op(1, 1), 1)],
                pad: 0,
                symbols: setup,
                target: 0,
            },
            commit(1, 2, 100),
        ],
    )])];
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("a", vec![big(2, "alpha")]),
            ProcPlan::new("b", vec![big(3, "beta")]),
            ProcPlan::new("w", vec![commit(4, 1, 100)]),
        ]),
        doctor(),
    ];
    sc
}

/// Writers whose phase 3 runs the checkpoint trigger ([F16] P-51, [F17 §5]).
pub fn auto() -> Scenario {
    let mut sc = Scenario::new("auto");
    sc.cfg.auto_checkpoint_bytes = 1_000;
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 700)]),
            ProcPlan::new("w2", vec![commit(2, 1, 700)]),
        ]),
        doctor(),
    ];
    sc
}

/// Two maintenance processes beside a writer: only one holds the maintenance byte at a time ([F16] P-76). Pauses (FM-6)
/// may stop a maintenance run between its decision and its append while the other decides later and appends first.
pub fn maint2() -> Scenario {
    let mut sc = Scenario::new("maint2");
    sc.rates.pause = 50_000;
    sc.phases = vec![
        phase(vec![ProcPlan::new("w", vec![commit(3, 1, 300)])]),
        phase(vec![
            ProcPlan::new("ma", vec![Act::Checkpoint]),
            ProcPlan::new("mb", vec![commit(5, 1, 300), Act::Checkpoint]),
            ProcPlan::new("w2", vec![commit(4, 1, 300)]),
        ]),
        doctor(),
    ];
    sc
}

/// A reader that starts after the writers' acknowledgements and replays the whole log (P-57, P-58, P-92's reader rule).
pub fn reads() -> Scenario {
    let mut sc = Scenario::new("reads");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 300)]),
            ProcPlan::new("w2", vec![commit(2, 1, 300)]),
        ]),
        phase(vec![ProcPlan::new("r", vec![Act::Read { views: 1 }])]),
    ];
    sc
}

/// Two claims of one task: the second finds the first pending under the writer byte and is refused, then reads (P-30,
/// P-34).
pub fn overlay() -> Scenario {
    let t = uid(9, 1);
    let mut sc = Scenario::new("overlay");
    sc.phases = vec![phase(vec![
        ProcPlan::new(
            "w1",
            vec![Act::Claim {
                op: op(1, 1),
                uid: t,
            }],
        ),
        ProcPlan::new(
            "w2",
            vec![
                Act::Claim {
                    op: op(2, 1),
                    uid: t,
                },
                Act::Read { views: 1 },
            ],
        ),
        ProcPlan::new("w3", vec![commit(3, 1, 200)]),
    ])];
    sc
}

/// A lazy group published, then made part of a flushed `HEAD` by durable publishes that flush no log (`quiet on`, then
/// `quiet off`), and lost at a scripted reboot while `HEAD` keeps it; after the reboot a writer appends below the old
/// `committed_lsn` beside a reader (P-49, P-64). The flag ends off: a `repair` of `HEAD` takes the flags from the newest
/// extent head ([F16] P-85), which holds none set after it.
pub fn lazytail() -> Scenario {
    let mut sc = Scenario::new("lazytail");
    sc.phases = vec![
        phase(vec![ProcPlan::new(
            "l",
            vec![
                commit(1, 1, 300),
                rt(1, 2, 0xE1, 2_000),
                Act::Quiet {
                    op: op(1, 3),
                    on: true,
                },
                Act::Quiet {
                    op: op(1, 4),
                    on: false,
                },
            ],
        )]),
        Phase {
            before: vec![DriverAct::Crash],
            procs: vec![
                ProcPlan::new("w", vec![commit(2, 1, 300)]),
                ProcPlan::new("r", vec![Act::Read { views: 2 }]),
            ],
        },
    ];
    sc
}

/// Four writers of equal-sized commits: after a failed flush their pending groups may be lost and the hole refilled by
/// groups of the same length (P-46, P-47, P-53); `doctor --verify` then checks that every acknowledged group is still
/// behind the predecessor it was validated against (I-G3).
pub fn lost() -> Scenario {
    let mut sc = Scenario::new("lost");
    sc.phases = vec![
        phase(vec![
            ProcPlan::new("w1", vec![commit(1, 1, 300), commit(1, 2, 300)]),
            ProcPlan::new("w2", vec![commit(2, 1, 300), commit(2, 2, 300)]),
            ProcPlan::new("w3", vec![commit(3, 1, 300)]),
            ProcPlan::new("w4", vec![commit(4, 1, 300)]),
        ]),
        doctor(),
    ];
    sc
}

/// `config_gen` bumps beside a writer and lazy appenders whose publishes interleave with them (P-4, P-20, P-38, P-48).
pub fn bump() -> Scenario {
    let mut sc = Scenario::new("bump");
    sc.phases = vec![phase(vec![
        ProcPlan::new("w", vec![commit(1, 1, 200)]),
        ProcPlan::new("a", vec![Act::Bump, Act::Bump, Act::Bump]),
        ProcPlan::new(
            "l1",
            vec![
                rt(2, 1, 0xB1, 100),
                rt(2, 2, 0xB2, 100),
                rt(2, 3, 0xB3, 100),
            ],
        ),
        ProcPlan::new("l2", vec![rt(3, 1, 0xB4, 100), rt(3, 2, 0xB5, 100)]),
    ])];
    sc
}

/// Every scenario of the clean suite.
pub fn all() -> Vec<Scenario> {
    vec![
        basic(),
        leases(),
        rotate(),
        spare(),
        retire(),
        ckpt(),
        gc(),
        fork(),
        intents(),
        trash(),
        xvol(),
        boot(),
        reboot(),
        init_store(),
        inproc(),
        admin(),
        import(),
        server(),
        timeouts(),
        retry(),
        intents_live(),
        clock(),
        quiet(),
        sizes(),
        auto(),
        maint2(),
        reads(),
        overlay(),
        lazytail(),
        lost(),
        bump(),
        boundary(),
        reuse(),
        stale(),
        barrier(),
        fatal(),
        misplaced(),
        foreign(),
        flags(),
    ]
}
