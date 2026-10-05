//! `doctor --verify` of the toy: the toy vehicle's `model` family ([F16 §17.2]: the toy has no reference model, since
//! PLAN §2.2 keeps the comparison with `moirai-model` out of the toy harness, so `doctor --verify` re-derives from the
//! log's records the invariants that comparison would find broken). The checks read the raw facts alone
//! ([`crate::state::Fact`]: the records' fields in log order, recorded as each record is decoded), never the replayed
//! tables, except where a check compares what the facts imply with what the toy's replay holds (I27′'s tips). The toy
//! writes them and R-HARN-S reviews them at WP-40's acceptance ([F16 §17.2] "Where the detectors live for the toy
//! vehicle"). [`crate::Toy::doctor_state`] replays the facts from the log's and the `hist` files' bytes, not from the
//! segment snapshots.
//!
//! - I1 (one uid per `#N`, one `#N` per uid);
//! - I14′ (an idempotent retry appends once: one commit per keyed operation, one `Idem` record per key);
//! - I27′ with [F05 §10.3] and [F16] P-69, P-70 (a commit implies its own ref move, CAS-checked in log order, whatever
//!   flag its record carries, and a commit whose CAS failed is parked, never moved: every ref's replayed tip is the tip
//!   this CAS rule gives over the facts). The toy's `RefUpdate` reason 6 ([`crate::format::REASON_MOVE`]), which only
//!   the seeded bugs of P-69 and P-70 write, is no record form of the specification: it changes no tip of the CAS rule
//!   and is never itself reported ([F16 §17.2]: a toy check judges the specification's rules over the record forms it
//!   defines). The tip check finds P-69 in the states where a crash or a process death cuts the log between a commit's
//!   group and its move's later group, and P-70 in every state that adopted the move;
//! - P-52 with [F05 §4.7] over the one kind of marker entry the toy writes ([F16 §17.2] "minimal markers"): a
//!   completion's `settled` entry ([RULES/state-definition] ME-001, MF-009) names a commit of its own group, and a
//!   lease released by `complete` ([F05 §9.4] reason 2) has its task's entry in its group, so a completion is never
//!   adopted without its marker. The rule is the toy's restriction of [F05 §4.7]; `GroupFacts` states its limits;
//! - I17′ with [OS/clock §4.3] (one live claim per task; a claim reclaims only a lease whose deadline passed on the boot
//!   clock, [F16] P-89; fencing tokens increase);
//! - I43′ with [F16] P-36 (the HLC sequence over the semantic records, and each local commit's `hlc` from it);
//! - the ref-id allocator ([F04 §5.14]: a ref id is created once);
//! - P-50 with I-G6 (the segment set's coverage never decreases: a set change's `upto_lsn` is at least the previous set
//!   change's, which a second maintenance holder's stale set change breaks, [F16] P-76);
//! - P-81 (a fork from a pinned checkpoint set has its `Pin`).
//!
//! The facts exist only when the configuration keeps them ([`crate::Config::facts`]); on a state without them every
//! check is vacuous.

use std::collections::{BTreeMap, BTreeSet};

use moirai_vfs::{DeadlineState, Stamp, hlc_next};

use crate::format::{
    CK_SET_CHANGE, COMMIT_IMPORTED, LEASE_CLAIM, LEASE_RELEASE, REASON_CREATE, REASON_DELETE,
    REASON_PARK,
};
use crate::state::{Fact, State};

/// `Lease.reason` 2, `complete`: a release into `settled` by the commit that completes the task ([F05 §9.4]).
const RELEASE_COMPLETE: u8 = 2;

/// A ref as the `RefTable` and `RefUpdate` facts describe it.
#[derive(Clone, Copy, Debug, Default)]
struct RefFacts {
    rkind: u8,
    deleted: bool,
    base_pin: u64,
}

/// The records of one group that [F05 §4.7]'s composition rules relate ([F16] P-52).
///
/// The check covers the one kind of `Marker` entry the toy writes: the `settled` entry a completing commit originates
/// (mkind 1, written only by `complete`; [RULES/state-definition] ME-001, MF-009). Its limits against the
/// specification:
/// - **Stronger outside the toy's vocabulary.** A re-emit (ME-003, ME-006, ME-007) and the `Marker` entries of a ref
///   group ([F05 §4.7] ref group, [F05 §9.5] causes 2–5) name an older origin commit, which is not in their group; the
///   toy writes neither, so the rule "a marker entry names a commit of its own group" is not to be carried as it stands
///   to a vehicle that writes them. A `complete` whose commit lands on a non-work ref writes no marker (ME-008); the
///   toy has no such ref (no `plan/*`, `merge/*` or `import/*` ref takes a completing commit).
/// - **Weaker for a completion that releases no lease.** The toy writes a completing commit for a holder without a
///   live lease, which [API §10.5] refuses (E407). Its marker split into a later group shows here only as a marker
///   outside its commit's group; with that group lost there is nothing to see in the facts, and the enumerator's
///   all-or-nothing verdict finds it instead (the harness declares the `Marker` effect in the commit's `begin`).
#[derive(Debug, Default)]
struct GroupFacts {
    /// Its first byte.
    start: u64,
    /// The ops of its `Commit` records.
    commits: BTreeSet<u64>,
    /// Its `Marker` entries: (lsn, task, origin commit).
    markers: Vec<(u64, u64, u64)>,
    /// Its leases released by `complete`: (lsn, task).
    completions: Vec<(u64, u64)>,
}

impl GroupFacts {
    /// P-52's findings over the group: a completion's `settled` entry whose origin commit is not in the group, and a
    /// completion without its task's entry. A completion's marker in a later group is lost with that group when a crash
    /// or a failed flush cuts the log between the two, which leaves the task's lease settled and the task open ([AR
    /// §4.3]).
    fn check(&self, out: &mut Vec<String>) {
        for &(lsn, uid, op) in &self.markers {
            if !self.commits.contains(&op) {
                out.push(format!(
                    "P-52: the Marker at {lsn} settles task {uid:#x} for commit {op:#x}, which is not in the marker's \
                     group (at {}); a completion's `settled` entry names a commit of its own group (ME-001, MF-009; \
                     [F05 §4.7]), and the toy writes no other entry",
                    self.start
                ));
            }
        }
        for &(lsn, uid) in &self.completions {
            if !self.markers.iter().any(|m| m.1 == uid) {
                out.push(format!(
                    "P-52: the lease of task {uid:#x} is released by `complete` at {lsn} without the task's Marker in \
                     its group (at {}); `complete`'s release (reason 2) and the `settled` entry of the task it settles \
                     are in the completing commit's group ([F05 §4.7], [API §10.5])",
                    self.start
                ));
            }
        }
    }
}

/// The violations of the invariants in `state`'s facts; empty for a correct store.
pub fn verify(state: &State) -> Vec<String> {
    let mut out = Vec::new();
    // I1: #N ↔ uid.
    let mut by_n: BTreeMap<u32, u64> = BTreeMap::new();
    let mut by_uid: BTreeMap<u64, u32> = BTreeMap::new();
    // I14′: one commit per keyed op, one Idem record per key. An operation is keyed when an `Idem` record names it
    // ([F05 §9.6]); an unkeyed one (the recovering writer's probe) may commit twice, since P-47 re-runs it after an
    // identity check that a read error failed while its first group was already durable.
    let keyed: BTreeSet<u64> = state
        .facts
        .iter()
        .filter_map(|f| match f {
            Fact::Idem { op, .. } if *op != 0 => Some(*op),
            _ => None,
        })
        .collect();
    let mut ops: BTreeSet<u64> = BTreeSet::new();
    let mut keys: BTreeSet<u64> = BTreeSet::new();
    // I27′: the ref tips by the CAS rule; the refs as their records describe them.
    let mut tips: BTreeMap<u32, u64> = BTreeMap::new();
    let mut created: BTreeSet<u32> = BTreeSet::new();
    let mut refs: BTreeMap<u32, RefFacts> = BTreeMap::new();
    // P-52: the group whose facts are being read (none before the first group's bounds).
    let mut group: Option<GroupFacts> = None;
    // P-81: the pins by holder ref.
    let mut pins: BTreeMap<u32, u64> = BTreeMap::new();
    // I17′: the live claim per task (lease id, token, deadline), and the last token.
    let mut live: BTreeMap<u64, (u64, u64, Stamp)> = BTreeMap::new();
    let mut last_token = 0u64;
    // I43′, P-36.
    let mut h_seq = 0u64;
    let mut h_commit = 0u64;
    // P-50: the coverage of the newest set change so far.
    let mut coverage: Option<u64> = None;
    for f in &state.facts {
        match f {
            Fact::Commit { lsn, rec } => {
                for &(n, uid) in &rec.creates {
                    if let Some(&u) = by_n.get(&n)
                        && u != uid
                    {
                        out.push(format!(
                            "I1: #{n} names two nodes ({u:#x} and {uid:#x}); commit at {lsn}"
                        ));
                    }
                    if let Some(&m) = by_uid.get(&uid)
                        && m != n
                    {
                        out.push(format!(
                            "I1: uid {uid:#x} has two #N (#{m} and #{n}); commit at {lsn}"
                        ));
                    }
                    by_n.entry(n).or_insert(uid);
                    by_uid.entry(uid).or_insert(n);
                }
                if !ops.insert(rec.op) && keyed.contains(&rec.op) {
                    out.push(format!(
                        "I14′: the operation {:#x} is committed twice (a retry appended again); commit at {lsn}",
                        rec.op
                    ));
                }
                if rec.flags & COMMIT_IMPORTED != 0 {
                    let want = hlc_next(rec.wall_ms as i64, h_seq);
                    if rec.append_hlc != want {
                        out.push(format!(
                            "P-36: the imported commit at {lsn} has append_hlc {:#x}, the HLC sequence gives {want:#x}",
                            rec.append_hlc
                        ));
                    }
                } else {
                    let want = hlc_next(rec.wall_ms as i64, h_seq.max(h_commit));
                    if rec.hlc != want || rec.append_hlc != rec.hlc {
                        out.push(format!(
                            "P-36 (I43′): the commit at {lsn} has hlc {:#x}, the HLC sequence gives {want:#x}; its \
                             commit id differs from the model's",
                            rec.hlc
                        ));
                    }
                }
                h_seq = h_seq.max(rec.append_hlc);
                h_commit = h_commit.max(rec.hlc);
                // P-69: adopting the commit implies its ref move, CAS-checked against the tips in log order ([F05
                // §10.3]); no flag of the record suspends it (the toy's flags bit 1 exists only in P-69's seeded bug).
                if created.contains(&rec.ref_id)
                    && tips.get(&rec.ref_id).copied().unwrap_or(0) == rec.ref_old
                {
                    tips.insert(rec.ref_id, rec.op);
                }
                if let Some(g) = &mut group {
                    g.commits.insert(rec.op);
                }
            }
            Fact::RefUpdate { lsn, rec } => {
                if rec.hlc <= h_seq {
                    out.push(format!(
                        "I43′: the ref update at {lsn} does not advance the HLC sequence"
                    ));
                }
                h_seq = h_seq.max(rec.hlc);
                match rec.reason {
                    REASON_CREATE | REASON_PARK if !created.contains(&rec.ref_id) => {
                        created.insert(rec.ref_id);
                        tips.insert(rec.ref_id, rec.new);
                    }
                    REASON_CREATE => {
                        out.push(format!(
                            "F04 §5.14: ref id {} is created twice (at {lsn}); ref ids are never reused",
                            rec.ref_id
                        ));
                        tips.insert(rec.ref_id, rec.new);
                    }
                    REASON_PARK => {
                        tips.insert(rec.ref_id, rec.new);
                    }
                    REASON_DELETE => {
                        refs.entry(rec.ref_id).or_default().deleted = true;
                    }
                    // The toy's reason 6 ([`crate::format::REASON_MOVE`]: a commit's move written apart from it, P-69;
                    // a failed CAS's move, P-70) changes no tip of the CAS rule, which moves a ref only for a commit
                    // whose CAS holds ([F05 §10.3], I27′); the replay applies it, and the tip check below compares. It
                    // is no record form of the specification and is not itself reported ([F16 §17.2]).
                    _ => {}
                }
            }
            Fact::RefTable { rec, .. } => {
                for e in &rec.entries {
                    let r = refs.entry(e.ref_id).or_default();
                    r.rkind = e.rkind;
                    r.deleted = e.eflags & 1 != 0;
                    r.base_pin = e.base_pin;
                }
            }
            Fact::Lease { lsn, rec } => {
                if rec.hlc <= h_seq {
                    out.push(format!(
                        "I43′: the lease record at {lsn} does not advance the HLC sequence"
                    ));
                }
                h_seq = h_seq.max(rec.hlc);
                match rec.event {
                    LEASE_CLAIM => {
                        if rec.token <= last_token {
                            out.push(format!(
                                "I17′: the claim at {lsn} takes fencing token {} after {last_token}",
                                rec.token
                            ));
                        }
                        last_token = last_token.max(rec.token);
                        if let Some(&(id, _, expires)) = live.get(&rec.uid) {
                            if rec.reclaimed != id {
                                out.push(format!(
                                    "I17′: the claim at {lsn} takes task {:#x} while lease {id} is live (two exclusive \
                                     claims)",
                                    rec.uid
                                ));
                            } else if expires.state(&rec.decided_at) == DeadlineState::NotPassed {
                                out.push(format!(
                                    "P-89: the claim at {lsn} reclaims lease {id} of task {:#x}, whose deadline has not \
                                     passed on the boot clock",
                                    rec.uid
                                ));
                            }
                        }
                        live.insert(rec.uid, (rec.lease_id, rec.token, rec.expires));
                    }
                    LEASE_RELEASE => {
                        // A release names its lease, not its task ([F05 §9.4]): the task is the live claim's. A
                        // lease claimed before the facts begin names no task here, and its completion is not judged.
                        if rec.reason == RELEASE_COMPLETE
                            && let Some(g) = &mut group
                            && let Some((&uid, _)) = live
                                .iter()
                                .find(|(_, v)| v.0 == rec.lease_id && v.1 == rec.token)
                        {
                            g.completions.push((*lsn, uid));
                        }
                        live.retain(|_, v| !(v.0 == rec.lease_id && v.1 == rec.token));
                    }
                    _ => {}
                }
            }
            Fact::Semantic { lsn, hlc } => {
                if *hlc <= h_seq {
                    out.push(format!(
                        "I43′: the record at {lsn} does not advance the HLC sequence"
                    ));
                }
                h_seq = h_seq.max(*hlc);
            }
            Fact::Marker { lsn, entry } => {
                if entry.hlc <= h_seq {
                    out.push(format!(
                        "I43′: the record at {lsn} does not advance the HLC sequence"
                    ));
                }
                h_seq = h_seq.max(entry.hlc);
                if let Some(g) = &mut group {
                    g.markers.push((*lsn, entry.uid, entry.op));
                }
            }
            Fact::Group { start, .. } => {
                if let Some(g) = group.take() {
                    g.check(&mut out);
                }
                group = Some(GroupFacts {
                    start: *start,
                    ..GroupFacts::default()
                });
            }
            Fact::Idem { lsn, key, .. } => {
                if !keys.insert(*key) {
                    out.push(format!(
                        "I14′: idempotency key {key:#x} is recorded twice (at {lsn}); a duplicate commit"
                    ));
                }
            }
            Fact::Checkpoint { lsn, rec } => {
                // [F16] P-50 and I-G6: checkpoint_lsn never decreases, so neither does the coverage of the segment
                // sets the set changes publish. The toy's rebuild of a damaged base segment ([80 §2.5] rule 8) keeps
                // its set's bound; no set change lowers it.
                if rec.ckflags & CK_SET_CHANGE != 0 {
                    if let Some(c) = coverage
                        && rec.upto_lsn < c
                    {
                        out.push(format!(
                            "P-50: the set change at {lsn} covers up to {}, below the previous set change's {c}; the \
                             segment sets' coverage decreased",
                            rec.upto_lsn
                        ));
                    }
                    coverage = Some(coverage.map_or(rec.upto_lsn, |c| c.max(rec.upto_lsn)));
                }
            }
            Fact::Pin { rec, .. } => {
                if rec.op == 1 {
                    pins.insert(rec.ref_id, rec.set_lsn);
                } else {
                    pins.remove(&rec.ref_id);
                }
            }
        }
    }
    if let Some(g) = group {
        g.check(&mut out);
    }
    // P-81: a fork that starts from a pinned checkpoint set has its `Pin` (in the fork's own group).
    for (id, r) in &refs {
        if r.deleted || r.base_pin == 0 {
            continue;
        }
        if pins.get(id) != Some(&r.base_pin) {
            out.push(format!(
                "P-81: fork ref {id} starts from the checkpoint set at {}, and no Pin holds that set",
                r.base_pin
            ));
        }
    }
    // I27′: every ref's replayed tip is the one the CAS rule gives over the facts.
    for id in &created {
        if refs.get(id).is_some_and(|r| r.rkind == 6) {
            continue;
        }
        let want = tips.get(id).copied().unwrap_or(0);
        if let Some(row) = state.refs.get(id)
            && row.tip != want
        {
            out.push(format!(
                "I27′: ref {id}'s tip is {:#x}; the ref CAS rule gives {want:#x}",
                row.tip
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{
        CheckpointRec, CommitRec, LeaseRec, PinRec, REASON_MOVE, RefEntry, RefTableRec,
        RefUpdateRec, SegRef, family,
    };
    use crate::state::RefRow;

    const WALL: u64 = 1_790_000_000_000;

    /// A local commit on ref 0 with the HLC the sequence gives after `prev` (the largest HLC so far).
    fn commit(op: u64, ref_old: u64, creates: Vec<(u32, u64)>, prev: u64) -> CommitRec {
        let h = hlc_next(WALL as i64, prev);
        CommitRec {
            op,
            digest: op,
            seq: op,
            ref_id: 0,
            ref_old,
            hlc: h,
            append_hlc: h,
            wall_ms: WALL,
            flags: 0,
            creates,
            filler: 0,
        }
    }

    fn create(ref_id: u32, hlc: u64) -> Fact {
        Fact::RefUpdate {
            lsn: 1,
            rec: RefUpdateRec {
                reason: REASON_CREATE,
                ref_id,
                op: 0,
                hlc,
                old: 0,
                new: 0,
            },
        }
    }

    fn table(ref_id: u32, rkind: u8, base_pin: u64) -> Fact {
        Fact::RefTable {
            lsn: 2,
            rec: RefTableRec {
                entries: vec![RefEntry {
                    ref_id,
                    name: u64::from(ref_id) + 1,
                    rkind,
                    eflags: 0,
                    tip: 0,
                    base_pin,
                }],
            },
        }
    }

    /// A correct history: main created, two commits, a claim and its release, an idempotency record, a checkpoint and
    /// a fork with its pin. `tip`: the replayed tip of ref 0.
    fn good() -> (State, Vec<Fact>) {
        let h0 = hlc_next(WALL as i64, 0);
        let c1 = commit(10, 0, vec![(1, 0x100)], h0);
        let c2 = commit(11, 10, vec![(2, 0x200)], c1.hlc);
        let claim = LeaseRec {
            event: LEASE_CLAIM,
            lease_id: 1,
            token: 1,
            hlc: hlc_next(WALL as i64, c2.hlc),
            uid: 0x900,
            holder: 3,
            expires: Stamp::NEVER,
            ttl_ms: 0,
            decided_at: Stamp {
                wall: WALL,
                boot_hash: 0,
                boot_ns: 0,
            },
            reclaimed: 0,
            reason: 0,
        };
        let release = LeaseRec {
            event: LEASE_RELEASE,
            hlc: hlc_next(WALL as i64, claim.hlc),
            reason: 1,
            ..claim.clone()
        };
        let idem_hlc = hlc_next(WALL as i64, release.hlc);
        let ck = CheckpointRec {
            ckflags: CK_SET_CHANGE,
            append_hlc: hlc_next(WALL as i64, idem_hlc),
            next_file_no: 2,
            upto_lsn: 500,
            active_log: 1,
            segments: vec![SegRef {
                file_no: 1,
                kind: 1,
                upto_lsn: 500,
                digest: [0; 16],
            }],
            retirements: Vec::new(),
            released: Vec::new(),
        };
        let fork_hlc = hlc_next(WALL as i64, idem_hlc);
        let facts = vec![
            create(0, h0),
            table(0, 1, 0),
            Fact::Commit { lsn: 300, rec: c1 },
            Fact::Commit { lsn: 400, rec: c2 },
            Fact::Lease {
                lsn: 450,
                rec: claim,
            },
            Fact::Lease {
                lsn: 460,
                rec: release,
            },
            Fact::Semantic {
                lsn: 470,
                hlc: idem_hlc,
            },
            Fact::Idem {
                lsn: 470,
                key: 11,
                op: 11,
            },
            Fact::Checkpoint { lsn: 480, rec: ck },
            create(1, fork_hlc),
            table(1, 1, 480),
            Fact::Pin {
                lsn: 490,
                rec: PinRec {
                    op: 1,
                    holder: 1,
                    ref_id: 1,
                    set_lsn: 480,
                    files: vec![(family::SEG_BASE, 1)],
                },
            },
        ];
        let mut st = State::new().keeping_facts(true);
        st.refs.insert(
            0,
            RefRow {
                name: 1,
                rkind: 1,
                tip: 11,
                ..RefRow::default()
            },
        );
        (st, facts)
    }

    fn run(facts: Vec<Fact>) -> Vec<String> {
        let (mut st, _) = good();
        st.facts = facts;
        verify(&st)
    }

    fn assert_one(v: &[String], prefix: &str) {
        assert!(
            v.len() == 1 && v[0].starts_with(prefix),
            "expected one {prefix} finding, got {v:?}"
        );
    }

    #[test]
    fn a_correct_history_has_no_finding() {
        let (_, facts) = good();
        assert_eq!(run(facts), Vec::<String>::new());
        // Without facts every check is vacuous.
        assert!(verify(&State::new()).is_empty());
    }

    #[test]
    fn i1_two_nodes_for_one_number_or_two_numbers_for_one_uid() {
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.creates = vec![(1, 0x999)];
        }
        assert_one(&run(f), "I1: #1 names two nodes");
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.creates = vec![(7, 0x100)];
        }
        assert_one(&run(f), "I1: uid 0x100 has two #N");
    }

    #[test]
    fn i14_a_keyed_retry_committed_twice_or_a_key_recorded_twice() {
        // Operation 11 carries the idempotency key 11: a second commit of it is a duplicate.
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[2] {
            rec.op = 11;
        }
        let v = run(f);
        assert!(
            v.iter().any(|m| m.starts_with("I14′: the operation 0xb")),
            "{v:?}"
        );
        // Operation 10 carries no key: P-47 may re-run it after a failed identity check, so a second commit is not one.
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.op = 10;
        }
        assert!(run(f).iter().all(|m| !m.starts_with("I14′")));
        let (_, mut f) = good();
        f.insert(
            8,
            Fact::Idem {
                lsn: 475,
                key: 11,
                op: 11,
            },
        );
        assert_one(&run(f), "I14′: idempotency key 0xb");
    }

    #[test]
    fn p36_a_commit_hlc_off_the_sequence() {
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.hlc += 1;
            rec.append_hlc += 1;
        }
        let v = run(f);
        assert!(v.iter().any(|m| m.starts_with("P-36 (I43′)")), "{v:?}");
        // An imported commit keeps its own hlc; only its append_hlc follows the sequence.
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.flags = COMMIT_IMPORTED;
            rec.hlc = 5;
        }
        assert!(run(f).iter().all(|m| !m.starts_with("P-36")));
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.flags = COMMIT_IMPORTED;
            rec.append_hlc += 1;
        }
        assert!(
            run(f)
                .iter()
                .any(|m| m.starts_with("P-36: the imported commit"))
        );
    }

    #[test]
    fn i43_records_that_do_not_advance_the_sequence() {
        let (_, mut f) = good();
        if let Fact::Semantic { hlc, .. } = &mut f[6] {
            *hlc = 1;
        }
        assert!(
            run(f)
                .iter()
                .any(|m| m.starts_with("I43′: the record at 470"))
        );
        let (_, mut f) = good();
        if let Fact::Lease { rec, .. } = &mut f[4] {
            rec.hlc = 1;
        }
        assert!(
            run(f)
                .iter()
                .any(|m| m.starts_with("I43′: the lease record"))
        );
        let (_, mut f) = good();
        f.push(create(2, 1));
        assert!(run(f).iter().any(|m| m.starts_with("I43′: the ref update")));
    }

    #[test]
    fn i17_two_live_claims_a_live_reclaim_and_tokens() {
        let (_, mut f) = good();
        // Drop the release: a second claim of the task then takes it while lease 1 is live.
        f.remove(5);
        let Fact::Lease { rec: claim, .. } = &f[4] else {
            panic!("fact 4 is the claim: {:?}", f[4]);
        };
        let mut second = claim.clone();
        second.lease_id = 2;
        second.token = 2;
        second.hlc = u64::MAX - 10;
        f.push(Fact::Lease {
            lsn: 900,
            rec: second.clone(),
        });
        assert!(
            run(f.clone())
                .iter()
                .any(|m| m.contains("while lease 1 is live"))
        );
        // A reclaim of a lease whose deadline has not passed (P-89).
        second.reclaimed = 1;
        let last = f.len() - 1;
        f[last] = Fact::Lease {
            lsn: 900,
            rec: second.clone(),
        };
        assert!(run(f.clone()).iter().any(|m| m.starts_with("P-89")));
        // A token that does not increase.
        second.token = 1;
        second.reclaimed = 0;
        let (_, mut g) = good();
        g.push(Fact::Lease {
            lsn: 900,
            rec: LeaseRec {
                uid: 0x901,
                ..second
            },
        });
        assert!(
            run(g)
                .iter()
                .any(|m| m.contains("takes fencing token 1 after 1"))
        );
    }

    #[test]
    fn i27_a_failed_cas_moves_its_ref_or_the_replayed_tip_differs() {
        // A failed CAS's move (P-70's seeded bug writes the toy's reason 6): the replay applies it, and the CAS rule,
        // which it does not change, gives the old tip.
        let (mut st, mut f) = good();
        f.push(Fact::RefUpdate {
            lsn: 950,
            rec: RefUpdateRec {
                reason: REASON_MOVE,
                ref_id: 0,
                op: 12,
                hlc: u64::MAX - 1,
                old: 11,
                new: 12,
            },
        });
        st.facts = f;
        if let Some(r) = st.refs.get_mut(&0) {
            r.tip = 12;
        }
        assert_one(
            &verify(&st),
            "I27′: ref 0's tip is 0xc; the ref CAS rule gives 0xb",
        );
        // The toy-only record itself is never reported: with a replayed tip the CAS rule gives, nothing is found.
        st.refs.get_mut(&0).expect("ref 0").tip = 11;
        assert_eq!(verify(&st), Vec::<String>::new());
        // The replay put a tip the CAS rule does not give.
        let (mut st, f) = good();
        st.facts = f;
        if let Some(r) = st.refs.get_mut(&0) {
            r.tip = 10;
        }
        assert_one(&verify(&st), "I27′: ref 0's tip is 0xa");
        // A commit whose ref_old is stale does not move the model's tip.
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.ref_old = 99;
        }
        assert_one(&run(f), "I27′: ref 0's tip is 0xb");
    }

    #[test]
    fn p69_a_commit_implies_its_ref_move_whatever_its_flags() {
        use crate::format::COMMIT_DETACHED;
        // The second commit carries the toy's flags bit 1 (P-69's seeded bug): the CAS rule still moves the tip to it.
        // With the separate move lost (a crash between the two groups) the replay kept tip 10.
        let (mut st, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.flags = COMMIT_DETACHED;
        }
        st.facts = f.clone();
        if let Some(r) = st.refs.get_mut(&0) {
            r.tip = 10;
        }
        assert_one(
            &verify(&st),
            "I27′: ref 0's tip is 0xa; the ref CAS rule gives 0xb",
        );
        // With the separate move adopted the replay's tip is the moved commit, which the CAS rule gives too: nothing is
        // found, since the toy-only record is not itself reported. The bug shows only where the move's group is lost.
        f.push(Fact::RefUpdate {
            lsn: 960,
            rec: RefUpdateRec {
                reason: REASON_MOVE,
                ref_id: 0,
                op: 11,
                hlc: u64::MAX - 1,
                old: 10,
                new: 11,
            },
        });
        st.facts = f;
        st.refs.get_mut(&0).expect("ref 0").tip = 11;
        assert_eq!(verify(&st), Vec::<String>::new());
    }

    /// How [`completion`] lays out a completion's records.
    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Layout {
        /// The commit, its release by `complete` and its marker in one group ([F05 §4.7]).
        One,
        /// The marker in a group of its own after the commit's (P-52's seeded bug).
        Split,
        /// As `Split`, with the marker's group lost (a crash between the two groups).
        MarkerLost,
    }

    /// Main created, commit 10, a claim of task 0x900 by holder 3 (lease 1), then commit 11 completing the task:
    /// the release of lease 1 by `complete` and the task's marker, laid out by `layout`. Every record follows the HLC
    /// sequence, and the tips agree with [`good`]'s replayed tip 11.
    fn completion(layout: Layout) -> Vec<Fact> {
        let h0 = hlc_next(WALL as i64, 0);
        let c1 = commit(10, 0, vec![(1, 0x100)], h0);
        let claim = LeaseRec {
            event: LEASE_CLAIM,
            lease_id: 1,
            token: 1,
            hlc: hlc_next(WALL as i64, c1.hlc),
            uid: 0x900,
            holder: 3,
            expires: Stamp::NEVER,
            ttl_ms: 0,
            decided_at: Stamp::NEVER,
            reclaimed: 0,
            reason: 0,
        };
        let c2 = commit(11, 10, vec![(2, 0x200)], claim.hlc);
        // A release as the log holds it: its lease and token, no task ([F05 §9.4]).
        let release = LeaseRec::decode(
            &LeaseRec {
                event: LEASE_RELEASE,
                hlc: hlc_next(WALL as i64, c2.hlc),
                reason: RELEASE_COMPLETE,
                ..claim.clone()
            }
            .encode(),
        )
        .expect("a release record");
        assert_eq!(release.uid, 0);
        let m = Fact::Marker {
            lsn: 430,
            entry: crate::format::MarkerEntry {
                mkind: 1,
                uid: 0x900,
                ref_id: 0,
                op: 11,
                seq: 11,
                hlc: hlc_next(WALL as i64, release.hlc),
                status: 1,
            },
        };
        let mut out = vec![
            create(0, h0),
            table(0, 1, 0),
            Fact::Group {
                start: 300,
                end: 350,
            },
            Fact::Commit { lsn: 300, rec: c1 },
            Fact::Group {
                start: 350,
                end: 400,
            },
            Fact::Lease {
                lsn: 350,
                rec: claim,
            },
            Fact::Group {
                start: 400,
                end: 490,
            },
            Fact::Commit { lsn: 400, rec: c2 },
            Fact::Lease {
                lsn: 420,
                rec: release,
            },
        ];
        match layout {
            Layout::One => out.push(m),
            Layout::Split => {
                out.push(Fact::Group {
                    start: 490,
                    end: 560,
                });
                out.push(m);
            }
            Layout::MarkerLost => {}
        }
        out
    }

    #[test]
    fn p52_a_marker_outside_its_commit_group() {
        assert_eq!(run(completion(Layout::One)), Vec::<String>::new());
        // The marker in a group of its own: the completion has no marker in its group, and the marker names a commit
        // of another group.
        let v = run(completion(Layout::Split));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].starts_with(
            "P-52: the lease of task 0x900 is released by `complete` at 420 without the task's Marker in its group \
             (at 400)"
        ));
        assert!(v[1].starts_with(
            "P-52: the Marker at 430 settles task 0x900 for commit 0xb, which is not in the marker's group (at 490)"
        ));
        // The marker's group lost: the completion alone shows it.
        assert_one(
            &run(completion(Layout::MarkerLost)),
            "P-52: the lease of task 0x900 is released by `complete` at 420",
        );
        // A plain release (reason 1) implies no marker.
        let mut f = completion(Layout::MarkerLost);
        if let Some(Fact::Lease { rec, .. }) = f.last_mut() {
            rec.reason = 1;
        }
        assert_eq!(run(f), Vec::<String>::new());
        // A completion of a lease claimed before the facts begin (here: no claim of lease 1) names no task, and is not
        // judged.
        let mut f = completion(Layout::MarkerLost);
        for x in &mut f {
            if let Fact::Lease { lsn: 350, rec } = x {
                rec.lease_id = 7;
            }
        }
        assert_eq!(run(f), Vec::<String>::new());
        // Facts without group bounds (a state that kept none) give no composition finding.
        let f: Vec<Fact> = completion(Layout::Split)
            .into_iter()
            .filter(|x| !matches!(x, Fact::Group { .. }))
            .collect();
        assert_eq!(run(f), Vec::<String>::new());
        // A marker's HLC is in the HLC sequence (I43′, [F16] P-36).
        let mut f = completion(Layout::One);
        if let Some(Fact::Marker { entry, .. }) = f.last_mut() {
            entry.hlc = 1;
        }
        assert_one(&run(f), "I43′: the record at 430");
    }

    #[test]
    fn f04_a_ref_id_created_twice() {
        let (_, mut f) = good();
        f.push(create(0, u64::MAX - 1));
        let v = run(f);
        assert!(
            v.iter()
                .any(|m| m.starts_with("F04 §5.14: ref id 0 is created twice")),
            "{v:?}"
        );
    }

    #[test]
    fn p50_a_set_change_below_the_previous_coverage() {
        let lower = CheckpointRec {
            ckflags: CK_SET_CHANGE,
            append_hlc: 1,
            next_file_no: 3,
            upto_lsn: 200,
            active_log: 1,
            segments: vec![SegRef {
                file_no: 2,
                kind: 1,
                upto_lsn: 200,
                digest: [0; 16],
            }],
            retirements: Vec::new(),
            released: vec![(family::SEG_BASE, 1)],
        };
        // A set change below the previous one's 500 lowers checkpoint_lsn, whatever it releases.
        let (_, mut f) = good();
        f.push(Fact::Checkpoint {
            lsn: 990,
            rec: lower.clone(),
        });
        assert_one(&run(f), "P-50: the set change at 990");
        // The same bound (a rebuilt base segment) or a higher one is a legitimate change.
        for upto in [500, 700] {
            let (_, mut f) = good();
            f.push(Fact::Checkpoint {
                lsn: 990,
                rec: CheckpointRec {
                    upto_lsn: upto,
                    ..lower.clone()
                },
            });
            assert!(run(f).is_empty(), "{upto}");
        }
    }

    #[test]
    fn p81_a_fork_without_its_pin() {
        let (_, mut f) = good();
        f.pop();
        assert_one(&run(f), "P-81: fork ref 1");
        // An unpin ends the pin too.
        let (_, mut f) = good();
        f.push(Fact::Pin {
            lsn: 995,
            rec: PinRec {
                op: 2,
                holder: 1,
                ref_id: 1,
                set_lsn: 480,
                files: Vec::new(),
            },
        });
        assert_one(&run(f), "P-81: fork ref 1");
    }
}
