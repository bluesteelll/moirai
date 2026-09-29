//! The toy's write operations ([F16 §5]: every writer of durable records goes through phases 2a and 2b): what each one
//! decides from the state it reads (phase 1 at L0, re-validated under the writer byte by P-34) and the records it
//! appends (the group compositions of [F05 §4.7] and [F16] P-52).

use std::collections::BTreeMap;

use moirai_vfs::{Stamp, hlc_next};

use crate::bugs::{Bug, Bugs};
use crate::format::{
    COMMIT_IMPORTED, CheckpointRec, CommitRec, Counters, IdemRec, IntentAbortRec, IntentDoneRec,
    IntentRec, LEASE_CLAIM, LEASE_RELEASE, LeaseRec, MarkerEntry, MarkerRec, PinRec, REASON_CREATE,
    REASON_MOVE, REASON_PARK, Rec, RefEntry, RefTableRec, RefUpdateRec, RuntimeRec, kind,
};
use crate::state::State;

/// A commit on a ref.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitOp {
    /// The operation (the commit's identity; a retry carries the same value).
    pub op: u64,
    /// The content digest.
    pub digest: u64,
    /// The ref's name.
    pub ref_name: u64,
    /// The uids of the nodes it creates (a known uid keeps its `#N`, I1).
    pub creates: Vec<u64>,
    /// The task it completes: a `settled` marker in the commit's group ([F16] P-52), and the release of the task's lease
    /// held by `holder`.
    pub completes: Option<u64>,
    /// The holder whose lease `completes` releases.
    pub holder: u64,
    /// The idempotency key ([F16] P-32): an `Idem` record in the commit's group.
    pub key: Option<u64>,
    /// Filler bytes (the commit-size distribution).
    pub filler: u32,
    /// An import: the commit keeps this `ref_old` (and is parked if it does not match, I27′).
    pub import_old: Option<u64>,
    /// The `FsIntentDone` of a `file mv` or `file rm` commit: (intent lsn, recovered, outcomes).
    pub intent_done: Option<(u64, bool, Vec<u8>)>,
}

/// A lease claim on a task.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClaimOp {
    /// The operation.
    pub op: u64,
    /// The task's uid.
    pub uid: u64,
    /// The holder.
    pub holder: u64,
    /// The TTL in ms.
    pub ttl_ms: u64,
    /// The idempotency key.
    pub key: Option<u64>,
}

/// A lease release.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReleaseOp {
    /// The operation.
    pub op: u64,
    /// The task's uid.
    pub uid: u64,
    /// The holder.
    pub holder: u64,
}

/// A fork: a new ref created at another ref's tip, with the fork's `Pin` ([F16] P-81).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ForkOp {
    /// The operation.
    pub op: u64,
    /// The name of the ref forked from.
    pub from: u64,
    /// The new ref's name.
    pub name: u64,
}

/// A lazy runtime batch (a hook's evidence, a settle's rows).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeOp {
    /// The operation.
    pub op: u64,
    /// The rows: (key, value).
    pub rows: Vec<(u64, u64)>,
    /// Padding bytes (the batch's size).
    pub pad: u32,
    /// Symbols the rows use; new ones are defined in the record's `SymDefs` block, with ids allocated under the writer
    /// byte ([F05 §8.1] SD-4).
    pub symbols: Vec<String>,
    /// The phase-1 size bound of the batch (0: none): the batch is padded in phase 1 so its encoding is exactly this long
    /// with the symbol ids known at L0 (the setting of [F16] P-35's final check).
    pub target_len: u64,
}

/// One write operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Op {
    /// A commit.
    Commit(CommitOp),
    /// A lease claim.
    Claim(ClaimOp),
    /// A lease release.
    Release(ReleaseOp),
    /// A fork.
    Fork(ForkOp),
    /// A lazy runtime batch.
    Runtime(RuntimeOp),
    /// An `FsIntent` ([F16] P-16): its own durable group.
    Intent(IntentRec),
    /// An `FsIntentAborted`.
    Abort(IntentAbortRec),
    /// A `Checkpoint` (maintenance).
    Checkpoint(CheckpointRec),
    /// A ref move written apart from its commit (P-69's seeded bug only).
    Move {
        /// The commit's op.
        op: u64,
        /// The ref.
        ref_id: u32,
        /// The tip before.
        old: u64,
    },
    /// A fork's `Pin` written apart from the fork (P-81's seeded bug only): the fork's op and the record.
    Pin(u64, PinRec),
    /// The recovering writer's probe: a commit on the toy's hidden probe ref, which no effect reports.
    Probe(u64),
}

/// The toy's hidden probe ref name.
pub const PROBE_REF: u64 = u64::MAX;
/// The name of `orphans/<R>`: this base plus R.
pub const ORPHANS_BASE: u64 = 1 << 62;

impl Op {
    /// The operation's id.
    pub fn id(&self) -> u64 {
        match self {
            Op::Commit(c) => c.op,
            Op::Claim(c) => c.op,
            Op::Release(r) => r.op,
            Op::Fork(f) => f.op,
            Op::Runtime(r) => r.op,
            Op::Intent(i) => i.key,
            Op::Abort(a) => a.intent_lsn,
            Op::Checkpoint(c) => c.append_hlc,
            Op::Move { op, .. } => *op,
            Op::Pin(op, _) => *op,
            Op::Probe(op) => *op,
        }
    }

    /// The idempotency key, if the operation has one.
    pub fn key(&self) -> Option<u64> {
        match self {
            Op::Commit(c) => c.key,
            Op::Claim(c) => c.key,
            _ => None,
        }
    }

    /// The idempotency lookup of [F16] P-25, P-32 in `state`: the stored result and the lsn of the record that stores it.
    /// An `FsIntent` is found by its key (a re-run after a lost group finds the intent it appended, P-47).
    pub fn lookup(&self, state: &State) -> Option<(u64, u64)> {
        match self {
            Op::Intent(i) => state
                .intents
                .iter()
                .find(|(_, r)| r.rec.key == i.key)
                .map(|(&lsn, _)| (lsn, lsn)),
            _ => {
                let key = self.key()?;
                state.idem.get(&key).map(|row| (row.result, row.lsn))
            }
        }
    }

    /// Whether the operation's group is durable (its records' class).
    pub fn durable(&self, bugs: Bugs) -> bool {
        match self {
            Op::Runtime(_) => false,
            Op::Claim(_) | Op::Release(_) => !bugs.on(Bug::P05LeaseTaggedLazy),
            _ => true,
        }
    }
}

/// What an operation decided against the state it read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// A commit on ref `ref_id` whose tip is `ref_old`.
    Commit {
        /// The ref.
        ref_id: u32,
        /// Its tip.
        ref_old: u64,
    },
    /// A claim, releasing `reclaim` as dead first when non-zero.
    Claim {
        /// The dead lease it releases (0 = none).
        reclaim: u64,
        /// The stamp it decided at.
        now: Stamp,
    },
    /// A release of lease `lease_id` with `token`.
    Release {
        /// The lease.
        lease_id: u64,
        /// Its token.
        token: u64,
    },
    /// A fork of ref `from` at `tip`, pinning the set `set_lsn` with `files`.
    Fork {
        /// The ref forked from.
        from: u32,
        /// Its tip.
        tip: u64,
        /// The pinned set.
        set_lsn: u64,
        /// Its files.
        files: Vec<(u8, u32)>,
    },
    /// A runtime batch with the padding phase 1 sized it with (fixed content, never re-decided).
    Runtime {
        /// The padding of its last row.
        pad: u32,
    },
    /// Nothing to decide.
    Plain,
}

/// Why a decision refuses (exit 4, a conflict, or a missing ref).
pub type Refusal = &'static str;

/// The lease rule of [OS/clock §4.3] as the toy applies it: whether lease `row`'s deadline has passed at `now`, or its
/// boot changed. P-89's seeded bug evaluates the deadline on the wall clock on a known boot.
pub fn lease_dead(expires: &Stamp, now: &Stamp, bugs: Bugs) -> bool {
    if bugs.on(Bug::P89LeaseOnWallClock) {
        return now.wall >= expires.wall;
    }
    !matches!(expires.state(now), moirai_vfs::DeadlineState::NotPassed)
}

/// Decides `op` against `state` at the stamp `now`.
pub fn decide(op: &Op, state: &State, now: Stamp, bugs: Bugs) -> Result<Decision, Refusal> {
    Ok(match op {
        Op::Commit(c) => {
            // An intent is closed once ([F16] P-71: re-checked under the writer byte).
            if let Some((lsn, _, _)) = &c.intent_done
                && state
                    .intents
                    .get(lsn)
                    .is_some_and(|r| r.state != crate::state::IntentState::Open)
            {
                return Err("the intent is closed");
            }
            let ref_id = state.ref_id(c.ref_name).ok_or("no such ref")?;
            let tip = state.refs.get(&ref_id).map_or(0, |r| r.tip);
            Decision::Commit {
                ref_id,
                ref_old: c.import_old.unwrap_or(tip),
            }
        }
        Op::Probe(_) => match state.ref_id(PROBE_REF) {
            Some(ref_id) => Decision::Commit {
                ref_id,
                ref_old: state.refs.get(&ref_id).map_or(0, |r| r.tip),
            },
            None => Decision::Plain,
        },
        Op::Claim(c) => match state.live_lease(c.uid) {
            None => Decision::Claim { reclaim: 0, now },
            Some((id, row)) => {
                if lease_dead(&row.expires, &now, bugs) {
                    Decision::Claim { reclaim: id, now }
                } else {
                    return Err("the task is leased by another holder");
                }
            }
        },
        Op::Release(r) => match state.live_lease(r.uid) {
            Some((id, row)) if row.holder == r.holder => Decision::Release {
                lease_id: id,
                token: row.token,
            },
            _ => return Err("no lease of this holder"),
        },
        Op::Fork(f) => {
            let from = state.ref_id(f.from).ok_or("no such ref")?;
            if state.ref_id(f.name).is_some() {
                return Err("the ref exists");
            }
            let files = state
                .segments
                .iter()
                .map(|s| (crate::format::family::SEG_BASE, s.file_no))
                .collect();
            Decision::Fork {
                from,
                tip: state.refs.get(&from).map_or(0, |r| r.tip),
                set_lsn: state.set_lsn,
                files,
            }
        }
        Op::Abort(a) => {
            if state
                .intents
                .get(&a.intent_lsn)
                .is_some_and(|r| r.state != crate::state::IntentState::Open)
            {
                return Err("the intent is closed");
            }
            Decision::Plain
        }
        Op::Runtime(r) => Decision::Runtime {
            pad: runtime_pad(r, state, bugs),
        },
        Op::Move { .. } | Op::Pin(..) | Op::Intent(_) | Op::Checkpoint(_) => Decision::Plain,
    })
}

/// The allocators of [F16] P-31: the maximum of the newest slot's counters and what the scanned log implies.
#[derive(Clone, Debug)]
pub struct Alloc {
    /// The counters before the operation's records.
    pub before: Counters,
    /// The counters as the operation allocates.
    pub next: Counters,
}

impl Alloc {
    /// The allocators from the slot's counters and the scanned state's. P-31's seeded bug allocates `#N` from the slot
    /// alone.
    pub fn new(slot: &Counters, scanned: &Counters, bugs: Bugs) -> Alloc {
        let mut c = slot.max(*scanned);
        if bugs.on(Bug::P31AllocateFromHeadOnly) {
            c.next_id = slot.next_id;
        }
        Alloc { before: c, next: c }
    }

    /// The next `seq`.
    pub fn seq(&mut self) -> u64 {
        self.next.commit_seq += 1;
        self.next.commit_seq
    }

    /// The `#N` of `uid`: a known uid keeps its `#N` (I1), a new one takes `next_id`.
    pub fn node(&mut self, known: Option<u32>) -> u32 {
        if let Some(n) = known {
            return n;
        }
        let n = self.next.next_id;
        self.next.next_id += 1;
        n
    }

    /// The next fencing token.
    pub fn token(&mut self) -> u64 {
        self.next.fence += 1;
        self.next.fence
    }

    /// The next ref id.
    pub fn ref_id(&mut self) -> u32 {
        let r = self.next.next_ref_id;
        self.next.next_ref_id += 1;
        r
    }
}

/// The HLC rule of [F16] P-36 over the two maxima `h_seq` and `h_commit`.
#[derive(Clone, Debug)]
pub struct Hlc {
    /// The wall-clock reading at append.
    pub wall_ms: i64,
    /// `h_seq`.
    pub seq: u64,
    /// `h_commit`.
    pub commit: u64,
    bugs: Bugs,
}

impl Hlc {
    /// The rule at `wall_ms` from the two maxima.
    pub fn new(wall_ms: i64, seq: u64, commit: u64, bugs: Bugs) -> Hlc {
        Hlc {
            wall_ms,
            seq,
            commit,
            bugs,
        }
    }

    /// A semantic durable record's HLC; raises `h_seq`.
    pub fn semantic(&mut self) -> u64 {
        let v = hlc_next(self.wall_ms, self.seq);
        self.seq = v;
        v
    }

    /// A local commit's `hlc` (also its `append_hlc`): above every commit the store holds.
    pub fn local_commit(&mut self) -> u64 {
        let v = hlc_next(self.wall_ms, self.seq.max(self.commit));
        self.seq = v;
        self.commit = self.commit.max(v);
        v
    }

    /// An HLC carried outside the sequence (`Checkpoint.append_hlc`, lazy records): raises nothing. P-36's seeded bug
    /// raises `h_seq`.
    pub fn carried(&mut self) -> u64 {
        let v = hlc_next(self.wall_ms, self.seq);
        if self.bugs.on(Bug::P36CheckpointAdvancesHlc) {
            self.seq = v;
        }
        v
    }

    /// The wall reading as the commit records it.
    pub fn wall(&self) -> u64 {
        self.wall_ms.max(0) as u64
    }
}

/// What building an operation's records produced.
#[derive(Clone, Debug, Default)]
pub struct Built {
    /// The groups, in order.
    pub groups: Vec<Vec<Rec>>,
    /// The operation's result (the op for a commit, the token for a claim, the new ref id for a fork).
    pub result: u64,
    /// A group appended after the acknowledgement (P-69's seeded bug): the ref move.
    pub later: Option<Op>,
}

/// Builds the records of `op` under the writer byte, from `decision`, the allocators and the HLC.
pub fn build(
    op: &Op,
    d: &Decision,
    state: &State,
    alloc: &mut Alloc,
    hlc: &mut Hlc,
    bugs: Bugs,
) -> Result<Built, Refusal> {
    let rec = |k: u8, p: Vec<u8>| Rec::new(k, p, bugs);
    let mut out = Built::default();
    match (op, d) {
        (Op::Commit(c), Decision::Commit { ref_id, ref_old }) => {
            let seq = alloc.seq();
            let creates = c
                .creates
                .iter()
                .map(|&uid| (alloc.node(state.uids.get(&uid).copied()), uid))
                .collect();
            let (h, ah) = if c.import_old.is_some() {
                // An imported commit keeps its own hlc (here: its digest's low bits in the past) and takes only its
                // append_hlc from the sequence.
                let ah = hlc.semantic();
                (c.digest & 0xFFFF_FFFF, ah)
            } else {
                let v = hlc.local_commit();
                (v, v)
            };
            let detached = bugs.on(Bug::P69T2RefMoveInLaterGroup) && c.import_old.is_none();
            let commit = CommitRec {
                op: c.op,
                digest: c.digest,
                seq,
                ref_id: *ref_id,
                ref_old: *ref_old,
                hlc: h,
                append_hlc: ah,
                wall_ms: hlc.wall(),
                flags: if c.import_old.is_some() {
                    COMMIT_IMPORTED
                } else if detached {
                    crate::format::COMMIT_DETACHED
                } else {
                    0
                },
                creates,
                filler: c.filler,
            };
            let mut group = vec![rec(kind::COMMIT, commit.encode())];
            let mut marker_group = Vec::new();
            // P-52's seeded bug writes the marker in a group of its own after the commit's group; its HLC is then taken
            // in log order, after the records of the commit's group (P-36), so the only change is the split.
            let split = bugs.on(Bug::P52T10MarkerInOwnGroup);
            let marker = |hlc: &mut Hlc, uid: u64| {
                let m = MarkerRec {
                    entries: vec![MarkerEntry {
                        mkind: 1,
                        uid,
                        ref_id: *ref_id,
                        op: c.op,
                        seq,
                        hlc: hlc.semantic(),
                        status: 1,
                    }],
                };
                rec(kind::MARKER, m.encode())
            };
            if let Some(uid) = c.completes {
                if !split {
                    group.push(marker(hlc, uid));
                }
                // The lease `complete` presented is released into `settled` ([F05 §9.4] reason 2).
                if let Some((id, row)) = state.live_lease(uid)
                    && row.holder == c.holder
                {
                    let l = LeaseRec {
                        event: LEASE_RELEASE,
                        lease_id: id,
                        token: row.token,
                        hlc: hlc.semantic(),
                        uid,
                        holder: c.holder,
                        expires: Stamp::NEVER,
                        ttl_ms: 0,
                        decided_at: Stamp::NEVER,
                        reclaimed: 0,
                        reason: 2,
                    };
                    group.push(rec(kind::LEASE, l.encode()));
                }
            }
            if let Some(key) = c.key {
                let i = IdemRec {
                    key,
                    payload: c.digest,
                    ref_id: *ref_id,
                    iflags: 0,
                    op: c.op,
                    append_hlc: hlc.semantic(),
                    result: c.op,
                };
                group.push(rec(kind::IDEM, i.encode()));
            }
            if let Some((intent_lsn, recovered, outcomes)) = &c.intent_done {
                let x = IntentDoneRec {
                    intent_lsn: *intent_lsn,
                    dflags: u8::from(*recovered),
                    hlc: hlc.semantic(),
                    outcomes: outcomes.clone(),
                };
                group.push(rec(kind::FS_INTENT_DONE, x.encode()));
            }
            if split && let Some(uid) = c.completes {
                marker_group.push(marker(hlc, uid));
            }
            out.groups.push(group);
            if !marker_group.is_empty() {
                out.groups.push(marker_group);
            }
            if detached {
                out.later = Some(Op::Move {
                    op: c.op,
                    ref_id: *ref_id,
                    old: *ref_old,
                });
            }
            out.result = c.op;
        }
        (Op::Probe(op), Decision::Commit { ref_id, ref_old }) => {
            let seq = alloc.seq();
            let v = hlc.local_commit();
            let commit = CommitRec {
                op: *op,
                digest: *op,
                seq,
                ref_id: *ref_id,
                ref_old: *ref_old,
                hlc: v,
                append_hlc: v,
                wall_ms: hlc.wall(),
                flags: 0,
                creates: Vec::new(),
                filler: 0,
            };
            out.groups.push(vec![rec(kind::COMMIT, commit.encode())]);
            out.result = *op;
        }
        (Op::Probe(op), Decision::Plain) => {
            // The probe ref does not exist yet: create it (a ref group, [F05 §4.7]).
            let id = alloc.ref_id();
            let h = hlc.semantic();
            let u = RefUpdateRec {
                reason: REASON_CREATE,
                ref_id: id,
                op: *op,
                hlc: h,
                old: 0,
                new: 0,
            };
            let t = RefTableRec {
                entries: vec![RefEntry {
                    ref_id: id,
                    name: PROBE_REF,
                    rkind: 1,
                    ..RefEntry::default()
                }],
            };
            out.groups.push(vec![
                rec(kind::REF_UPDATE, u.encode()),
                rec(kind::REF_TABLE, t.encode()),
            ]);
            out.result = *op;
        }
        (Op::Claim(c), Decision::Claim { reclaim, now }) => {
            let token = alloc.token();
            let l = LeaseRec {
                event: LEASE_CLAIM,
                lease_id: token,
                token,
                hlc: hlc.semantic(),
                uid: c.uid,
                holder: c.holder,
                expires: now.after(core::time::Duration::from_millis(c.ttl_ms)),
                ttl_ms: c.ttl_ms,
                decided_at: *now,
                reclaimed: *reclaim,
                reason: 0,
            };
            let mut group = vec![rec(kind::LEASE, l.encode())];
            if let Some(key) = c.key {
                let i = IdemRec {
                    key,
                    payload: c.uid,
                    ref_id: 0,
                    iflags: 2,
                    op: 0,
                    append_hlc: hlc.semantic(),
                    result: token,
                };
                group.push(rec(kind::IDEM, i.encode()));
            }
            out.groups.push(group);
            out.result = token;
        }
        (Op::Release(r), Decision::Release { lease_id, token }) => {
            let l = LeaseRec {
                event: LEASE_RELEASE,
                lease_id: *lease_id,
                token: *token,
                hlc: hlc.semantic(),
                uid: r.uid,
                holder: r.holder,
                expires: Stamp::NEVER,
                ttl_ms: 0,
                decided_at: Stamp::NEVER,
                reclaimed: 0,
                reason: 1,
            };
            out.groups.push(vec![rec(kind::LEASE, l.encode())]);
            out.result = *lease_id;
        }
        (
            Op::Fork(f),
            Decision::Fork {
                from: _,
                tip,
                set_lsn,
                files,
            },
        ) => {
            let id = alloc.ref_id();
            let u = RefUpdateRec {
                reason: REASON_CREATE,
                ref_id: id,
                op: f.op,
                hlc: hlc.semantic(),
                old: 0,
                new: *tip,
            };
            let t = RefTableRec {
                entries: vec![RefEntry {
                    ref_id: id,
                    name: f.name,
                    rkind: 1,
                    eflags: 0,
                    tip: *tip,
                    base_pin: *set_lsn,
                }],
            };
            let pin = PinRec {
                op: 1,
                holder: 1,
                ref_id: id,
                set_lsn: *set_lsn,
                files: files.clone(),
            };
            let mut group = vec![
                rec(kind::REF_UPDATE, u.encode()),
                rec(kind::REF_TABLE, t.encode()),
            ];
            if bugs.on(Bug::P81PinInLaterGroup) {
                // P-81's seeded bug: the Pin follows in a later group, after the acknowledgement.
                out.groups.push(group);
                out.later = Some(Op::Pin(f.op, pin));
            } else {
                group.push(rec(kind::PIN, pin.encode()));
                out.groups.push(group);
            }
            out.result = u64::from(id);
        }
        (Op::Runtime(r), d) => {
            let pad = match d {
                Decision::Runtime { pad } => *pad,
                _ => r.pad,
            };
            let _ = hlc.carried();
            out.groups.push(vec![runtime_rec(r, state, pad, bugs)]);
            out.result = r.op;
        }
        (Op::Intent(i), _) => {
            let mut i = i.clone();
            i.hlc = hlc.semantic();
            out.groups.push(vec![rec(kind::FS_INTENT, i.encode())]);
            out.result = i.key;
        }
        (Op::Abort(a), _) => {
            let mut a = a.clone();
            a.hlc = hlc.semantic();
            out.groups
                .push(vec![rec(kind::FS_INTENT_ABORTED, a.encode())]);
            out.result = a.intent_lsn;
        }
        (Op::Checkpoint(c), _) => {
            let mut c = c.clone();
            c.append_hlc = hlc.carried();
            c.next_file_no = c.next_file_no.max(alloc.next.next_file_no);
            out.groups.push(vec![rec(kind::CHECKPOINT, c.encode())]);
            out.result = c.upto_lsn;
        }
        (Op::Pin(_, pin), _) => {
            out.groups.push(vec![rec(kind::PIN, pin.encode())]);
            out.result = u64::from(pin.ref_id);
        }
        (Op::Move { op, ref_id, old }, _) => {
            let u = RefUpdateRec {
                reason: REASON_MOVE,
                ref_id: *ref_id,
                op: *op,
                hlc: hlc.semantic(),
                old: *old,
                new: *op,
            };
            out.groups.push(vec![rec(kind::REF_UPDATE, u.encode())]);
            out.result = *op;
        }
        _ => return Err("the operation does not match its decision"),
    }
    Ok(out)
}

/// The record of runtime batch `r` with `pad` bytes on its last row; its new symbols are defined with the ids that
/// follow `state`'s ([F05 §8.1] SD-1).
pub fn runtime_rec(r: &RuntimeOp, state: &State, pad: u32, bugs: Bugs) -> Rec {
    let mut defs = Vec::new();
    let mut next = state.symbols.len() as u32 + 1;
    for s in &r.symbols {
        if !state.symbols.iter().any(|x| x == s)
            && !defs.iter().any(|(_, x): &(u32, String)| x == s)
        {
            defs.push((next, s.clone()));
            next += 1;
        }
    }
    let mut rows: Vec<(u64, u64, u32)> = r.rows.iter().map(|&(k, v)| (k, v, 0)).collect();
    if let Some(last) = rows.last_mut() {
        last.2 = pad;
    }
    let mut x = Rec::new(kind::FILE_OBS, RuntimeRec { rows }.encode(), bugs);
    x.symdefs = defs;
    x
}

/// The largest padding [`runtime_pad`] tries: the largest extent of [F17 §2.2] P01.
const MAX_PAD: u64 = 1 << 30;

/// Phase 1's padding of runtime batch `r` ([F16] P-35's setting): with `target_len` set, the largest padding whose
/// group is at most that long with the symbol ids `state` implies — exactly that long, or one byte shorter where the last
/// row's length prefix would gain a byte across it ([F01 §5.2]: the length grows by one per padding byte, and by two
/// where the prefix grows) — and 0 when even an unpadded group is longer; else `r.pad`.
pub fn runtime_pad(r: &RuntimeOp, state: &State, bugs: Bugs) -> u32 {
    if r.target_len == 0 {
        return r.pad;
    }
    let len = |pad: u32| runtime_rec(r, state, pad, bugs).len(true);
    let bare = len(0);
    if bare >= r.target_len {
        return 0;
    }
    // Every padding byte adds at least one byte, so this start is at or above the answer; the length is monotone in
    // the padding, and each step down takes back one or two bytes. A start beyond the largest extent ([F17 §2.2]
    // P01: 2^30) gives a group that W3 refuses anyway.
    let mut pad = u32::try_from((r.target_len - bare).min(MAX_PAD)).unwrap_or(0);
    while pad > 0 && len(pad) > r.target_len {
        pad -= 1;
    }
    pad
}

/// The park group of [F16] P-70 for the failing commit `op` of ref `origin`: a `RefUpdate` reason 5 moving
/// `orphans/<R>` to it, with its `RefTable` entry (a full entry when the record creates the ref). `parked` holds the
/// `orphans/<R>` refs (id, tip) that earlier parks of the same append created or moved, which `state` does not show yet:
/// a second park of one origin moves the ref the first created instead of creating another. P-70's seeded bug moves the
/// commit's own ref instead.
pub fn park_group(
    op: u64,
    origin: u32,
    state: &State,
    alloc: &mut Alloc,
    hlc: &mut Hlc,
    bugs: Bugs,
    parked: &mut BTreeMap<u32, (u32, u64)>,
) -> Vec<Rec> {
    let rec = |k: u8, p: Vec<u8>| Rec::new(k, p, bugs);
    if bugs.on(Bug::P70FailedCasMovesRef) {
        let old = state.refs.get(&origin).map_or(0, |r| r.tip);
        let u = RefUpdateRec {
            reason: REASON_MOVE,
            ref_id: origin,
            op,
            hlc: hlc.semantic(),
            old,
            new: op,
        };
        return vec![rec(kind::REF_UPDATE, u.encode())];
    }
    let name = ORPHANS_BASE + u64::from(origin);
    let (id, old) = match (parked.get(&origin), state.ref_id(name)) {
        (Some(&(id, tip)), _) => (id, tip),
        (None, Some(id)) => (id, state.refs.get(&id).map_or(0, |r| r.tip)),
        (None, None) => (alloc.ref_id(), 0),
    };
    parked.insert(origin, (id, op));
    let u = RefUpdateRec {
        reason: REASON_PARK,
        ref_id: id,
        op,
        hlc: hlc.semantic(),
        old,
        new: op,
    };
    let t = RefTableRec {
        entries: vec![RefEntry {
            ref_id: id,
            name,
            rkind: 6,
            eflags: 0,
            tip: op,
            base_pin: 0,
        }],
    };
    vec![
        rec(kind::REF_UPDATE, u.encode()),
        rec(kind::REF_TABLE, t.encode()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn batch(rows: usize, symbols: &[&str], target: u64) -> RuntimeOp {
        RuntimeOp {
            op: 1,
            rows: (0..rows as u64).map(|k| (k, k)).collect(),
            pad: 0,
            symbols: symbols.iter().map(|s| (*s).to_owned()).collect(),
            target_len: target,
        }
    }

    /// The padding is the largest whose group is at most the target, and the group is the target or one byte short.
    fn check_pad(r: &RuntimeOp, st: &State) {
        let len = |p: u32| runtime_rec(r, st, p, Bugs::NONE).len(true);
        let pad = runtime_pad(r, st, Bugs::NONE);
        let bare = len(0);
        if bare >= r.target_len {
            assert_eq!(pad, 0);
            return;
        }
        let got = len(pad);
        assert!(
            got == r.target_len || got + 1 == r.target_len,
            "target {} gave {got} with pad {pad}",
            r.target_len
        );
        assert!(len(pad + 1) > r.target_len, "pad {pad} is not the largest");
    }

    #[test]
    fn runtime_pad_converges_across_length_prefix_boundaries() {
        let st = State::new();
        // The last row is 17 + pad bytes: its length prefix grows at 128 and 16,384 bytes.
        for rows in [1usize, 3] {
            for target in (40..400).chain(16_300..16_500) {
                check_pad(&batch(rows, &[], target), &st);
                check_pad(&batch(rows, &["alpha", "beta"], target), &st);
            }
        }
        // A target the bare group already exceeds, and one beyond the largest extent.
        assert_eq!(runtime_pad(&batch(50, &[], 60), &st, Bugs::NONE), 0);
        let far = batch(1, &[], u64::MAX);
        assert_eq!(runtime_pad(&far, &st, Bugs::NONE), MAX_PAD as u32);
        // Without a target the batch keeps its own padding.
        let mut own = batch(1, &[], 0);
        own.pad = 77;
        assert_eq!(runtime_pad(&own, &st, Bugs::NONE), 77);
    }

    #[test]
    fn two_parks_of_one_origin_share_its_orphans_ref() {
        let mut st = State::new();
        st.counters.next_ref_id = 1;
        let mut alloc = Alloc::new(&st.counters, &st.counters, Bugs::NONE);
        let mut hlc = Hlc::new(1_790_000_000_000, 0, 0, Bugs::NONE);
        let mut parked = BTreeMap::new();
        let a = park_group(10, 0, &st, &mut alloc, &mut hlc, Bugs::NONE, &mut parked);
        let b = park_group(11, 0, &st, &mut alloc, &mut hlc, Bugs::NONE, &mut parked);
        let ua = RefUpdateRec::decode(&a[0].payload).unwrap();
        let ub = RefUpdateRec::decode(&b[0].payload).unwrap();
        assert_eq!(
            (ua.reason, ua.ref_id, ua.old, ua.new),
            (REASON_PARK, 1, 0, 10)
        );
        assert_eq!(
            (ub.reason, ub.ref_id, ub.old, ub.new),
            (REASON_PARK, 1, 10, 11)
        );
        assert!(ub.hlc > ua.hlc);
        assert_eq!(alloc.next.next_ref_id, 2, "one orphans ref for one origin");
        // P-70's seeded bug moves the commit's own ref instead.
        let c = park_group(
            12,
            0,
            &st,
            &mut alloc,
            &mut hlc,
            Bugs::only(Bug::P70FailedCasMovesRef),
            &mut parked,
        );
        let uc = RefUpdateRec::decode(&c[0].payload).unwrap();
        assert_eq!((uc.reason, uc.ref_id), (REASON_MOVE, 0));
    }

    #[test]
    fn a_claim_on_a_live_lease_is_refused_and_a_dead_one_reclaimed() {
        let mut st = State::new();
        let now = Stamp {
            wall: 1_000,
            boot_hash: 0,
            boot_ns: 0,
        };
        let claim = Op::Claim(ClaimOp {
            op: 1,
            uid: 9,
            holder: 3,
            ttl_ms: 1000,
            key: None,
        });
        assert_eq!(
            decide(&claim, &st, now, Bugs::NONE),
            Ok(Decision::Claim { reclaim: 0, now })
        );
        st.leases.insert(
            5,
            crate::state::LeaseRow {
                token: 5,
                uid: 9,
                holder: 4,
                expires: Stamp::NEVER,
                released: false,
            },
        );
        st.lease_of.insert(9, 5);
        assert!(decide(&claim, &st, now, Bugs::NONE).is_err());
        // A lease whose deadline passed is reclaimed.
        if let Some(l) = st.leases.get_mut(&5) {
            l.expires = Stamp {
                wall: 500,
                boot_hash: 0,
                boot_ns: 0,
            };
        }
        assert_eq!(
            decide(&claim, &st, now, Bugs::NONE),
            Ok(Decision::Claim { reclaim: 5, now })
        );
        let release = Op::Release(ReleaseOp {
            op: 2,
            uid: 9,
            holder: 4,
        });
        assert_eq!(
            decide(&release, &st, now, Bugs::NONE),
            Ok(Decision::Release {
                lease_id: 5,
                token: 5
            })
        );
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn runtime_pad_is_the_largest_fitting_padding(rows in 1usize..6, target in 30u64..70_000) {
            check_pad(&batch(rows, &["s"], target), &State::new());
        }
    }
}
