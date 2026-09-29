//! `doctor --verify` of the toy: the invariants a reference model would compare ([F16 §17.2] "model"), re-derived from
//! the raw facts of the log alone ([`crate::state::Fact`]: the records' fields in log order), never from the replayed
//! tables, except where a check compares what the facts imply with what the toy's replay holds (I27′'s tips):
//!
//! - I1 (one uid per `#N`, one `#N` per uid);
//! - I14′ (an idempotent retry appends once: one commit per operation, one `Idem` record per key);
//! - I27′ (a commit whose ref CAS failed is parked and never moves its ref; every ref's replayed tip is the tip the CAS
//!   rule gives over the facts);
//! - I17′ with [OS/clock §4.3] (one live claim per task; a claim reclaims only a lease whose deadline passed on the boot
//!   clock, [F16] P-89; fencing tokens increase);
//! - I43′ with [F16] P-36 (the HLC sequence over the semantic records, and each local commit's `hlc` from it);
//! - the ref-id allocator ([F04 §5.14]: a ref id is created once);
//! - P-76 (a `Checkpoint`'s set change releases the set it replaces, or never covers less than it);
//! - P-81 (a fork from a pinned checkpoint set has its `Pin`).
//!
//! The facts exist only when the configuration keeps them ([`crate::Config::facts`]); on a state without them every
//! check is vacuous.

use std::collections::{BTreeMap, BTreeSet};

use moirai_vfs::{DeadlineState, Stamp, hlc_next};

use crate::format::{
    CK_SET_CHANGE, COMMIT_DETACHED, COMMIT_IMPORTED, LEASE_CLAIM, LEASE_RELEASE, REASON_CREATE,
    REASON_DELETE, REASON_MOVE, REASON_PARK, SegRef, family,
};
use crate::state::{Fact, State};

/// A ref as the `RefTable` and `RefUpdate` facts describe it.
#[derive(Clone, Copy, Debug, Default)]
struct RefFacts {
    rkind: u8,
    deleted: bool,
    base_pin: u64,
}

/// The violations of the invariants in `state`'s facts; empty for a correct store.
pub fn verify(state: &State) -> Vec<String> {
    let mut out = Vec::new();
    // I1: #N ↔ uid.
    let mut by_n: BTreeMap<u32, u64> = BTreeMap::new();
    let mut by_uid: BTreeMap<u64, u32> = BTreeMap::new();
    // I14′: one commit per op, one Idem record per key.
    let mut ops: BTreeSet<u64> = BTreeSet::new();
    let mut keys: BTreeSet<u64> = BTreeSet::new();
    // I27′: the ref tips by the CAS rule; the refs as their records describe them.
    let mut tips: BTreeMap<u32, u64> = BTreeMap::new();
    let mut created: BTreeSet<u32> = BTreeSet::new();
    let mut detached: BTreeSet<u64> = BTreeSet::new();
    let mut refs: BTreeMap<u32, RefFacts> = BTreeMap::new();
    // P-81: the pins by holder ref.
    let mut pins: BTreeMap<u32, u64> = BTreeMap::new();
    // I17′: the live claim per task (lease id, token, deadline), and the last token.
    let mut live: BTreeMap<u64, (u64, u64, Stamp)> = BTreeMap::new();
    let mut last_token = 0u64;
    // I43′, P-36.
    let mut h_seq = 0u64;
    let mut h_commit = 0u64;
    // P-76: the segment set the facts have published so far.
    let mut set: Vec<SegRef> = Vec::new();
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
                if !ops.insert(rec.op) {
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
                if rec.flags & COMMIT_DETACHED != 0 {
                    detached.insert(rec.op);
                } else if created.contains(&rec.ref_id)
                    && tips.get(&rec.ref_id).copied().unwrap_or(0) == rec.ref_old
                {
                    tips.insert(rec.ref_id, rec.op);
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
                    REASON_MOVE => {
                        if !detached.contains(&rec.new) {
                            out.push(format!(
                                "I27′: ref {} moved to {:#x} at {lsn} by a record apart from a commit whose CAS failed; \
                                 the commit must be parked on orphans/<R>",
                                rec.ref_id, rec.new
                            ));
                        }
                        tips.insert(rec.ref_id, rec.new);
                    }
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
            Fact::Idem { lsn, key, .. } => {
                if !keys.insert(*key) {
                    out.push(format!(
                        "I14′: idempotency key {key:#x} is recorded twice (at {lsn}); a duplicate commit"
                    ));
                }
            }
            Fact::Checkpoint { lsn, rec } => {
                if rec.ckflags & CK_SET_CHANGE != 0 {
                    let mut replaced: Vec<u32> = set.iter().map(|s| s.file_no).collect();
                    let mut released: Vec<u32> = rec
                        .released
                        .iter()
                        .filter(|&&(f, _)| f == family::SEG_BASE)
                        .map(|&(_, n)| n)
                        .collect();
                    replaced.sort_unstable();
                    released.sort_unstable();
                    let covered = set.iter().map(|s| s.upto_lsn).max().unwrap_or(0);
                    if replaced != released && covered > rec.upto_lsn {
                        out.push(format!(
                            "P-76: the Checkpoint at {lsn} replaces a segment set covering up to {covered}, which it does \
                             not release, by one covering up to {}; a concurrent checkpoint's delta is dropped",
                            rec.upto_lsn
                        ));
                    }
                    set.clone_from(&rec.segments);
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
        CheckpointRec, CommitRec, LeaseRec, PinRec, RefEntry, RefTableRec, RefUpdateRec,
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
    fn i14_a_retry_committed_twice_or_a_key_recorded_twice() {
        let (_, mut f) = good();
        if let Fact::Commit { rec, .. } = &mut f[3] {
            rec.op = 10;
        }
        let v = run(f);
        assert!(
            v.iter().any(|m| m.starts_with("I14′: the operation 0xa")),
            "{v:?}"
        );
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
        let (_, mut f) = good();
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
        let v = run(f);
        assert!(
            v.iter().any(|m| m.starts_with("I27′: ref 0 moved")),
            "{v:?}"
        );
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
    fn p76_a_set_change_that_drops_a_larger_set() {
        let (_, mut f) = good();
        let smaller = CheckpointRec {
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
            released: Vec::new(),
        };
        f.push(Fact::Checkpoint {
            lsn: 990,
            rec: smaller.clone(),
        });
        assert_one(&run(f), "P-76");
        // Releasing the replaced set is a legitimate change, whatever it covers.
        let (_, mut f) = good();
        f.push(Fact::Checkpoint {
            lsn: 990,
            rec: CheckpointRec {
                released: vec![(family::SEG_BASE, 1)],
                ..smaller
            },
        });
        assert!(run(f).is_empty());
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
