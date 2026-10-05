//! The assertions after every crash state: acknowledged durable effects, all-or-nothing operations (markers with their
//! commit), and read freshness ([60 §3.13] GT1; [60 §4.4] items 3–5; [F13 §3.8] I-G1, I-G2, I-G3,
//! I-G5).
//!
//! The recovered state must be the result of applying, in some order that respects real time, a subset of the begun
//! operations that contains every *required* one:
//! - an operation is **done** once it was acknowledged, or once a reader saw a value only it wrote;
//! - a lazy operation's effects **may vanish** if a system crash, or a failed flush counted after it began, came before
//!   the recovery: a lazy effect survives every process death ([F15 §4.1]), but not FM-1, and FM-3.6 lets a failed
//!   flush in any process lose a published lazy record — the failed flush may follow the effect even when it is counted
//!   before the acknowledgement, so the count at `begin` decides (FM-3.6 "Harness allowance", spec sync 2b S2B-P-13);
//! - so may the effects of a durable publish of fields kept only in `HEAD` ([`super::Ledger::begin_kept_in_head`]: a
//!   flag, `boot_id`) that was not acknowledged, under the same two conditions: a failed flush counted after it began
//!   poisons the slot sectors, so reads may alternate and a crash may leave either value (FM-3.6 "Fields kept only in
//!   `HEAD`", spec sync 2b S2B-P-14), and a system crash may revert its slot writes, which readers see before its `HEAD`
//!   flush ([F04 §9.2]; FM-1.1). A reader's observation of such a value therefore does not make the operation required;
//!   once acknowledged, after its `HEAD` flush, it is durable like any other;
//! - a done operation is **required** unless its effects may vanish (an ordinary durable operation's never do);
//! - operation A **may follow** B unless A was done before B began (A took effect before its `done`).
//!
//! The checks below are necessary conditions of such an order, so a correct subject never fails them:
//! 1. **Per register.** Its recovered value comes from an operation that writes it and may follow every required writer
//!    of it, or it is clear and no required operation writes it. Otherwise an acknowledged effect is lost, or the value
//!    is a phantom.
//! 2. **All or nothing.** An operation that is the only possible source of some recovered value took effect, so each of
//!    its other registers holds its value or a value of an operation that may follow it (a commit adopted without its
//!    group's markers fails here, [72 M1]).
//! 3. **Read freshness.** The first read after the crash, before any writer runs, passes 1 and 2 on its own, and recovery
//!    changes a register it read only to the value of an operation that may follow the one it saw — unless what it saw
//!    may vanish: a clear register, or a value that a lazy operation, or an unacknowledged durable publish of fields
//!    kept only in `HEAD`, whose effects may vanish wrote (FM-3.2: the first reader may read a poisoned sector's newest
//!    bytes and the recovering writer its oldest; FM-3.6 lets the lazy record, and the `HEAD`-only field, go). A reader
//!    serving a durable-class view that recovery then drops, or missing an acknowledged effect, fails here.
//!
//! **I-G2's post-crash clause, read precisely** ([F13 §3.8]; spec sync 2b S2B-P-18):
//! - **Boot mode.** After a crash the first read is judged complete for a Known-boot reader. An Unknown-boot reader
//!   ([`super::BootMode::Unknown`], from [`super::Subject::recover_with_boot`]) reflects every acknowledged record only
//!   after the next writer's flush ([OS/proc §5] U5–U6, [F16 §10] P-67), so its first read is judged for consistency
//!   only (checks 1 and 2 without required operations, and check 3); the recovered state is judged as always.
//! - **"Before any writer runs".** The judged first read is the reader's first read after its own boot-change recovery
//!   ([F16 §8] P-60, [F16 §10] P-66): [`super::Recovered::first_read`].
//! - **Process deaths without a crash.** The first read is judged for consistency only (the next paragraph).
//! - **Lazy values.** A lazy value is exempt once a flush failed after its operation began, counted store-wide (FM-3.6
//!   "Harness allowance"), and so is an unacknowledged field kept only in `HEAD` (the may-vanish rules above).
//!
//! **After process deaths alone** (no system crash) the first read is judged for consistency — checks 1 and 2 without
//! required operations (no phantom value, nothing partly applied) and check 3 — but not for completeness. I-G2 requires
//! "every read reflects every acknowledged record before any writer runs" after a crash ([F13 §3.8]); after deaths
//! alone a reader's view may lag behind the last publish until a writer publishes again: a dead writer's publish in
//! flight applied nothing after an overlapping read had seen it (§2.5, FM-4.1), or a failed flush at a death poisoned
//! the published slot, whose reads then return its older bytes (FM-3.2, N-12) — and a writer role byte held beyond every
//! bound or never released keeps every writer out (FM-8.1). The writer's recovered state stays complete: whatever a
//! reader saw was durable and survives.
//!
//! **Refusals** (avail, [F16 §17.2]): a refused first read, a refused recovery and every refusal the recovery answered
//! with repair are judged by module `refusal` against the faults of the run up to the recovery's end; the writer's final
//! answer may not be a refusal that the operator's repair answers. A refusal ends the checks of the state it replaces.

use std::collections::BTreeMap;
use std::path::Path;

use super::ledger::{Class, EffectKey, EffectSet, OpTable};
use super::refusal::{Allow, Facts, Setup};
use super::{BootMode, Recovered};

/// A possible source of a register's recovered value.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Cand {
    /// No operation wrote it (it is clear).
    Init,
    /// Operation `i` of the table.
    Op(usize),
}

/// The judge of one crash state against a ledger prefix.
pub(crate) struct Verdict<'a> {
    t: &'a OpTable,
    /// A system crash happened (lazy effects may be lost).
    crash: bool,
    /// The failed-flush count at the crash (or at the end of the run).
    ff_end: u64,
    /// The faults of the run up to the recovery's end, which decide the correct refusals (module `refusal`); `None`: a
    /// run with none.
    facts: Option<&'a Facts>,
    /// A role byte that a dead process holds beyond every wait bound or never releases (FM-8.1 classes (b), (c)): the
    /// recovery's bounded waits may run out.
    busy: bool,
    /// The setup's declarations and the slot files (module `refusal`).
    setup: Setup<'a>,
    /// Whether a path names a sealed file in the recovered world (a damaged one is derived, and repair rebuilds it).
    sealed: Option<&'a dyn Fn(&Path) -> bool>,
    /// The boot mode of the first read's reader: an Unknown-boot reader's first read is never judged complete.
    boot: BootMode,
}

impl<'a> Verdict<'a> {
    pub(crate) fn new(t: &'a OpTable, crash: bool, ff_end: u64) -> Verdict<'a> {
        Verdict {
            t,
            crash,
            ff_end,
            facts: None,
            busy: false,
            setup: Setup::default(),
            sealed: None,
            boot: BootMode::Known,
        }
    }

    /// The first read was made by a reader in boot mode `boot` (Known unless set).
    pub(crate) fn first_read_boot(mut self, boot: BootMode) -> Verdict<'a> {
        self.boot = boot;
        self
    }

    /// Judges refusals against the run's `facts`; `busy` as for [`Allow::busy`]; `setup`: the setup's declarations
    /// and the slot files; `sealed`: whether a path names a sealed file.
    pub(crate) fn faults(
        mut self,
        facts: &'a Facts,
        busy: bool,
        setup: Setup<'a>,
        sealed: &'a dyn Fn(&Path) -> bool,
    ) -> Verdict<'a> {
        self.facts = Some(facts);
        self.busy = busy;
        self.setup = setup;
        self.sealed = Some(sealed);
        self
    }

    /// Whether the store must keep some operation (acknowledged or seen, and not of a kind that may vanish).
    pub(crate) fn any_required(&self) -> bool {
        (0..self.t.ops.len()).any(|i| self.required(i))
    }

    /// The allowance for this recovery's refusals, over `facts`.
    fn allow<'b>(&'b self, facts: &'b Facts) -> Allow<'b> {
        Allow {
            facts,
            busy: self.busy,
            workload: false,
            nothing_required: !self.any_required(),
            setup: self.setup,
        }
    }

    /// The recovery's refusals that are correct answers (for the report's count).
    pub(crate) fn allowed_refusals(&self, rec: &Recovered) -> u64 {
        let none = Facts::default();
        let allow = self.allow(self.facts.unwrap_or(&none));
        let not_sealed = |_: &Path| false;
        let sealed = self.sealed.unwrap_or(&not_sealed);
        let answered = rec
            .answered
            .iter()
            .filter(|r| allow.check(r).is_ok())
            .count() as u64;
        let first = rec
            .first_read
            .as_ref()
            .err()
            .is_some_and(|r| allow.check(r).is_ok());
        let state = rec
            .state
            .as_ref()
            .err()
            .is_some_and(|r| allow.check_final(r, sealed).is_ok());
        answered + u64::from(first) + u64::from(state)
    }

    /// Whether operation `i`'s effects may vanish (see the module documentation): a lazy operation's, or an
    /// unacknowledged `HEAD`-only durable publish's, after a system crash or a failed flush counted after it began.
    fn may_vanish(&self, i: usize) -> bool {
        let o = &self.t.ops[i];
        let exposed = o.class == Class::Lazy || (o.head && o.acked.is_none());
        exposed && (self.crash || o.begin_ff < self.ff_end)
    }

    fn required(&self, i: usize) -> bool {
        self.t.ops[i].done.is_some() && !self.may_vanish(i)
    }

    /// Whether operation `a` may have taken effect after operation `b`.
    fn may_follow(&self, a: usize, b: usize) -> bool {
        let (a, b) = (&self.t.ops[a], &self.t.ops[b]);
        !a.done.is_some_and(|d| d < b.begin)
    }

    /// The valid sources of `value` in register `key` (check 1); `complete`: every required writer counts.
    fn cands(&self, key: &EffectKey, value: Option<u64>, complete: bool) -> Vec<Cand> {
        let writers: &[(usize, Option<u64>)] = self.t.by_key.get(key).map_or(&[], |w| w.as_slice());
        let required: Vec<usize> = writers
            .iter()
            .map(|&(i, _)| i)
            .filter(|&i| complete && self.required(i))
            .collect();
        let mut out = Vec::new();
        if value.is_none() && required.is_empty() {
            out.push(Cand::Init);
        }
        for &(i, v) in writers {
            if v == value && required.iter().all(|&y| y == i || self.may_follow(i, y)) {
                out.push(Cand::Op(i));
            }
        }
        out
    }

    fn explain(&self, label: &str, key: &EffectKey, value: Option<u64>) -> String {
        let writers: &[(usize, Option<u64>)] = self.t.by_key.get(key).map_or(&[], |w| w.as_slice());
        let shown = value.map_or_else(|| "clear".to_owned(), |v| format!("{v:#x}"));
        if value.is_some() && !writers.iter().any(|&(_, v)| v == value) {
            return format!("{label}: {key:?} = {shown}, a value no operation wrote (a phantom)");
        }
        let lost: Vec<String> = writers
            .iter()
            .filter(|&&(i, _)| self.required(i))
            .map(|&(i, v)| {
                let o = &self.t.ops[i];
                format!(
                    "op {} ({:?}, {}) wrote {}",
                    o.id,
                    o.class,
                    if o.done.is_some() {
                        "acknowledged or observed"
                    } else {
                        "pending"
                    },
                    v.map_or_else(|| "clear".to_owned(), |v| format!("{v:#x}"))
                )
            })
            .collect();
        format!(
            "{label}: {key:?} = {shown}, but no operation that may follow every required writer wrote it; required: {}",
            lost.join("; ")
        )
    }

    /// Checks 1 and 2 on one state; returns the messages and the valid sources of every register. `complete`: the state
    /// must hold every required operation (else only consistency is checked).
    fn check_set(
        &self,
        label: &str,
        set: &EffectSet,
        complete: bool,
    ) -> (Vec<String>, BTreeMap<EffectKey, Vec<Cand>>) {
        let mut msgs = Vec::new();
        let mut valid: BTreeMap<EffectKey, Vec<Cand>> = BTreeMap::new();
        let keys: Vec<EffectKey> = {
            let mut k: Vec<EffectKey> = self.t.by_key.keys().copied().collect();
            k.extend(set.keys().copied());
            k.sort_unstable();
            k.dedup();
            k
        };
        for key in keys {
            let value = set.get(&key).copied();
            let c = self.cands(&key, value, complete);
            if c.is_empty() {
                msgs.push(self.explain(label, &key, value));
            }
            valid.insert(key, c);
        }
        // Check 2: an operation that alone explains a recovered value took effect as a whole.
        for (i, o) in self.t.ops.iter().enumerate() {
            if complete && self.required(i) {
                continue;
            }
            let evidence = o
                .writes
                .iter()
                .find(|(k, _)| valid.get(k).is_some_and(|c| c.as_slice() == [Cand::Op(i)]));
            let Some(&(shown_key, _)) = evidence else {
                continue;
            };
            for &(k, _) in &o.writes {
                if k == shown_key {
                    continue;
                }
                let ok = valid.get(&k).is_some_and(|c| {
                    c.iter().any(|&c| match c {
                        Cand::Init => false,
                        Cand::Op(j) => j == i || self.may_follow(j, i),
                    })
                });
                if !ok {
                    msgs.push(format!(
                        "{label}: op {} ({:?}) is partly applied: {shown_key:?} shows it, {k:?} = {} does not",
                        o.id,
                        o.class,
                        set.get(&k)
                            .map_or_else(|| "clear".to_owned(), |v| format!("{v:#x}"))
                    ));
                }
            }
        }
        (msgs, valid)
    }

    /// All three checks on one recovery, and its refusals judged against the run's faults (module `refusal`).
    pub(crate) fn check(&self, rec: &Recovered) -> Vec<String> {
        let mut msgs: Vec<String> = rec
            .findings
            .iter()
            .map(|f| format!("subject: {f}"))
            .collect();
        let none = Facts::default();
        let allow = self.allow(self.facts.unwrap_or(&none));
        let not_sealed = |_: &Path| false;
        let sealed = self.sealed.unwrap_or(&not_sealed);
        for r in &rec.answered {
            if let Err(why) = allow.check(r) {
                msgs.push(format!(
                    "repair: the recovery answered a refusal with repair, {r}, that the run's faults do not explain: \
                     {why}"
                ));
            }
        }
        let first = match &rec.first_read {
            Ok(s) => {
                // Complete after a crash only, and only for a Known-boot reader (see the module documentation).
                let complete = self.crash && self.boot == BootMode::Known;
                let (m, v) = self.check_set("first read", s, complete);
                msgs.extend(m);
                Some((s, v))
            }
            Err(r) => {
                if let Err(why) = allow.check(r) {
                    msgs.push(format!("first read: refused: {r}: {why}"));
                }
                None
            }
        };
        let state = match &rec.state {
            Ok(s) => {
                let (m, v) = self.check_set("recovered state", s, true);
                msgs.extend(m);
                Some((s, v))
            }
            Err(r) => {
                if let Err(why) = allow.check_final(r, sealed) {
                    msgs.push(format!("recovery: refused: {r}: {why}"));
                }
                None
            }
        };
        if let (Some((fs, fv)), Some((ss, sv))) = (first, state) {
            let mut keys: Vec<EffectKey> = fs.keys().chain(ss.keys()).copied().collect();
            keys.sort_unstable();
            keys.dedup();
            for k in keys {
                let (f, s) = (fs.get(&k).copied(), ss.get(&k).copied());
                if f == s {
                    continue;
                }
                let (Some(fc), Some(sc)) = (fv.get(&k), sv.get(&k)) else {
                    continue;
                };
                if fc.is_empty() || sc.is_empty() {
                    // Already reported by check 1.
                    continue;
                }
                // What the reader saw may vanish: a clear register, or a value that a lazy operation whose effects may
                // vanish wrote (any such source explains the read, so the check stays a necessary condition).
                let vanished = fc.iter().any(|&f| match f {
                    Cand::Init => true,
                    Cand::Op(fi) => self.may_vanish(fi),
                });
                let followed = sc.iter().any(|&s| match s {
                    Cand::Init => false,
                    Cand::Op(si) => fc
                        .iter()
                        .any(|&f| matches!(f, Cand::Op(fi) if fi != si && self.may_follow(si, fi))),
                });
                if !(vanished || followed) {
                    let show = |v: Option<u64>| {
                        v.map_or_else(|| "clear".to_owned(), |v| format!("{v:#x}"))
                    };
                    msgs.push(format!(
                        "read freshness: a reader saw {k:?} = {} before any writer ran; recovery left {}",
                        show(f),
                        show(s)
                    ));
                }
            }
        }
        msgs
    }
}

#[cfg(test)]
mod tests {
    use super::super::ledger::{EffectKind, Entry, Rec};
    use super::super::refusal::{Refusal, Refused};
    use super::*;
    use crate::adversary::Site;
    use crate::trace::EventKind;

    const A: EffectKey = EffectKey::new(EffectKind::Commit, 1);
    const B: EffectKey = EffectKey::new(EffectKind::Commit, 2);
    const M: EffectKey = EffectKey::new(EffectKind::Marker, 1);
    const R: EffectKey = EffectKey::new(EffectKind::Ref, 7);

    fn e(at: u64, ff: u64, rec: Rec) -> Entry {
        Entry { at, ff, rec }
    }

    fn begin(op: u64, class: Class, w: &[(EffectKey, Option<u64>)]) -> Rec {
        Rec::Begin {
            op,
            class,
            head: false,
            writes: w.to_vec(),
        }
    }

    /// The `begin` of a durable publish of fields kept only in `HEAD` ([`super::super::Ledger::begin_kept_in_head`]).
    fn begin_head(op: u64, w: &[(EffectKey, Option<u64>)]) -> Rec {
        Rec::Begin {
            op,
            class: Class::Durable,
            head: true,
            writes: w.to_vec(),
        }
    }

    fn set(v: &[(EffectKey, u64)]) -> EffectSet {
        v.iter().copied().collect()
    }

    fn rec(first: EffectSet, state: EffectSet) -> Recovered {
        Recovered::same(EffectSet::new()).with(Ok(first), Ok(state))
    }

    impl Recovered {
        fn with(
            self,
            first_read: Result<EffectSet, Refused>,
            state: Result<EffectSet, Refused>,
        ) -> Recovered {
            Recovered {
                first_read,
                state,
                ..self
            }
        }
    }

    fn judge(entries: &[Entry], crash: bool, ff: u64, r: &Recovered) -> Vec<String> {
        let t = OpTable::build(entries, u64::MAX);
        Verdict::new(&t, crash, ff).check(r)
    }

    fn judge_boot(
        entries: &[Entry],
        crash: bool,
        ff: u64,
        boot: BootMode,
        r: &Recovered,
    ) -> Vec<String> {
        let t = OpTable::build(entries, u64::MAX);
        Verdict::new(&t, crash, ff).first_read_boot(boot).check(r)
    }

    #[test]
    fn an_acknowledged_durable_effect_must_survive_and_a_pending_one_may_go() {
        let log = [
            e(
                1,
                0,
                begin(10, Class::Durable, &[(A, Some(0xA)), (M, Some(0xAA))]),
            ),
            e(2, 0, Rec::Ack { op: 10 }),
            e(3, 0, begin(11, Class::Durable, &[(B, Some(0xB))])),
        ];
        let full = set(&[(A, 0xA), (M, 0xAA), (B, 0xB)]);
        assert!(judge(&log, true, 0, &rec(full.clone(), full)).is_empty());
        let without_b = set(&[(A, 0xA), (M, 0xAA)]);
        assert!(judge(&log, true, 0, &rec(without_b.clone(), without_b)).is_empty());
        let lost = set(&[(B, 0xB)]);
        let m = judge(&log, true, 0, &rec(lost.clone(), lost));
        assert!(m.iter().any(|m| m.contains("op 10")), "{m:?}");
        // A phantom value.
        let phantom = set(&[(A, 0xA), (M, 0xAA), (B, 0xBAD)]);
        let m = judge(&log, true, 0, &rec(phantom.clone(), phantom));
        assert!(m.iter().any(|m| m.contains("phantom")), "{m:?}");
    }

    #[test]
    fn a_commit_without_its_marker_is_partly_applied() {
        let log = [e(
            1,
            0,
            begin(10, Class::Durable, &[(A, Some(0xA)), (M, Some(0xAA))]),
        )];
        let half = set(&[(A, 0xA)]);
        let m = judge(&log, true, 0, &rec(half.clone(), half));
        assert!(m.iter().any(|m| m.contains("partly applied")), "{m:?}");
        assert!(judge(&log, true, 0, &rec(EffectSet::new(), EffectSet::new())).is_empty());
    }

    #[test]
    fn lazy_effects_survive_deaths_but_not_crashes_or_failed_flushes() {
        let log = [
            e(1, 0, begin(10, Class::Lazy, &[(A, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
        ];
        let none = rec(EffectSet::new(), EffectSet::new());
        assert!(
            judge(&log, true, 0, &none).is_empty(),
            "a crash may lose it"
        );
        assert!(
            judge(&log, false, 1, &none).is_empty(),
            "a later failed flush may lose it"
        );
        assert!(
            !judge(&log, false, 0, &none).is_empty(),
            "a death alone never loses it"
        );
    }

    #[test]
    fn registers_follow_real_time_order() {
        // Ref 7: op 1 sets 1 and is acknowledged; op 2 begins after that and sets 2 (pending); op 3 overlaps op 1.
        let log = [
            e(1, 0, begin(3, Class::Durable, &[(R, Some(3))])),
            e(1, 0, begin(1, Class::Durable, &[(R, Some(1))])),
            e(2, 0, Rec::Ack { op: 1 }),
            e(3, 0, begin(2, Class::Durable, &[(R, Some(2))])),
        ];
        for ok in [1, 2, 3] {
            let s = set(&[(R, ok)]);
            assert!(judge(&log, true, 0, &rec(s.clone(), s)).is_empty(), "{ok}");
        }
        // Op 3 acknowledged after op 1 but begun before its acknowledgement: either order is possible.
        let mut log2 = log.to_vec();
        log2.push(e(4, 0, Rec::Ack { op: 3 }));
        for ok in [1, 2, 3] {
            let s = set(&[(R, ok)]);
            assert!(judge(&log2, true, 0, &rec(s.clone(), s)).is_empty(), "{ok}");
        }
        // Clear is impossible once op 1 is acknowledged.
        assert!(!judge(&log, true, 0, &rec(EffectSet::new(), EffectSet::new())).is_empty());
        // An op begun after op 1's acknowledgement cannot precede it: once op 4 is acknowledged, 1 is no longer valid.
        let mut log3 = log.to_vec();
        log3.push(e(4, 0, begin(4, Class::Durable, &[(R, Some(4))])));
        log3.push(e(5, 0, Rec::Ack { op: 4 }));
        let s = set(&[(R, 1)]);
        assert!(!judge(&log3, true, 0, &rec(s.clone(), s)).is_empty());
        let s = set(&[(R, 2)]);
        assert!(
            judge(&log3, true, 0, &rec(s.clone(), s)).is_empty(),
            "op 2 overlaps op 4"
        );
    }

    #[test]
    fn an_observed_durable_value_counts_as_acknowledged() {
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(A, Some(0xA))])),
            e(
                2,
                0,
                Rec::Observe {
                    key: A,
                    value: Some(0xA),
                    covered: None,
                },
            ),
        ];
        let m = judge(&log, true, 0, &rec(EffectSet::new(), EffectSet::new()));
        assert!(m.iter().any(|m| m.contains("op 10")), "{m:?}");
        // A read of a value nobody wrote is a run problem.
        let t = OpTable::build(
            &[e(
                1,
                0,
                Rec::Observe {
                    key: A,
                    value: Some(1),
                    covered: None,
                },
            )],
            u64::MAX,
        );
        assert!(t.problems.iter().any(|p| p.contains("phantom")));
    }

    #[test]
    fn read_freshness_rejects_a_read_that_recovery_drops_and_a_stale_first_read() {
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(A, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
            e(3, 0, begin(11, Class::Durable, &[(B, Some(0xB))])),
        ];
        // The reader saw pending op 11; recovery dropped it.
        let r = rec(set(&[(A, 0xA), (B, 0xB)]), set(&[(A, 0xA)]));
        let m = judge(&log, true, 0, &r);
        assert!(m.iter().any(|m| m.contains("read freshness")), "{m:?}");
        // Recovery adopted pending op 11 that the reader had not seen: allowed.
        let r = rec(set(&[(A, 0xA)]), set(&[(A, 0xA), (B, 0xB)]));
        assert!(judge(&log, true, 0, &r).is_empty());
        // The first read misses the acknowledged op 10: stale.
        let r = rec(EffectSet::new(), set(&[(A, 0xA)]));
        let m = judge(&log, true, 0, &r);
        assert!(m.iter().any(|m| m.starts_with("first read")), "{m:?}");
    }

    #[test]
    fn a_refusal_is_a_failure_unless_allowed() {
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(A, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
        ];
        let t = OpTable::build(&log, u64::MAX);
        let io = || Err(Refused::new(Refusal::IoFault, "exit 7 store_io_fault"));
        let refused = Recovered::same(EffectSet::new()).with(io(), io());
        // A clean run explains neither.
        assert_eq!(Verdict::new(&t, true, 0).check(&refused).len(), 2);
        assert_eq!(Verdict::new(&t, true, 0).allowed_refusals(&refused), 0);
        // A persistent read error explains both (FM-12.4).
        let read = facts(&[(EventKind::Choice, Site::ReadFault as u64, 3, 2)]);
        let none = |_: &Path| false;
        let v = Verdict::new(&t, true, 0).faults(&read, false, Setup::default(), &none);
        assert!(v.check(&refused).is_empty(), "{:?}", v.check(&refused));
        assert_eq!(v.allowed_refusals(&refused), 2);
        // A truncated sealed file explains a damaged file, but the writer's final answer may not be one: repair
        // rebuilds a sealed file ([80 §2.5] rule 8).
        let seg = std::path::PathBuf::from("/s/seg.1");
        let damaged = || Err(Refused::new(Refusal::Damaged(seg.clone()), "exit 7"));
        let truncated = facts(&[
            (
                EventKind::Note,
                crate::trace::NOTE_PATH,
                9,
                crate::trace::path_hash("/s/seg.1"),
            ),
            (EventKind::External, 9, 0, 0),
        ]);
        let sealed = |p: &Path| p == Path::new("/s/seg.1");
        let v = Verdict::new(&t, true, 0).faults(&truncated, false, Setup::default(), &sealed);
        let m = v.check(&Recovered::same(EffectSet::new()).with(damaged(), damaged()));
        assert!(
            m.len() == 1 && m[0].starts_with("recovery: refused"),
            "{m:?}"
        );
        // A refusal the recovery answered with repair is judged too.
        let mut answered = rec(set(&[(A, 0xA)]), set(&[(A, 0xA)]));
        answered.answered = vec![Refused::new(Refusal::NoValidSlot, "exit 7")];
        let m = v.check(&answered);
        assert!(m.len() == 1 && m[0].starts_with("repair:"), "{m:?}");
        // E12: behind a byte a dead holder keeps, the first read's and the writer's bounded waits may run out.
        let busy = || Err(Refused::new(Refusal::Busy, "maintenance_busy"));
        let waits = Recovered::same(EffectSet::new()).with(busy(), busy());
        let clean = Facts::default();
        assert_eq!(
            Verdict::new(&t, false, 0)
                .faults(&clean, false, Setup::default(), &none)
                .check(&waits)
                .len(),
            2
        );
        let v = Verdict::new(&t, false, 0).faults(&clean, true, Setup::default(), &none);
        assert!(v.check(&waits).is_empty());
        // E13: outcome_unknown after a failed flush.
        let unknown = Recovered::same(EffectSet::new()).with(
            Ok(set(&[(A, 0xA)])),
            Err(Refused::new(
                Refusal::OutcomeUnknown,
                "exit 7 outcome_unknown",
            )),
        );
        let ff = facts(&[(EventKind::FlushEnd, 4, 0, 1)]);
        let v = Verdict::new(&t, false, 1).faults(&ff, false, Setup::default(), &none);
        assert!(v.check(&unknown).is_empty(), "{:?}", v.check(&unknown));
        assert!(!Verdict::new(&t, false, 0).check(&unknown).is_empty());
        // An answer is still judged.
        let wrong = Recovered::same(EffectSet::new()).with(io(), Ok(EffectSet::new()));
        let v = Verdict::new(&t, true, 0).faults(&read, false, Setup::default(), &none);
        assert!(!v.check(&wrong).is_empty());
    }

    /// The facts of a run with `events` (kind, a, b, c).
    fn facts(events: &[(EventKind, u64, u64, u64)]) -> Facts {
        let mut f = Facts::new(true);
        for &(kind, a, b, c) in events {
            f.feed(
                &crate::trace::Event {
                    kind,
                    task: 0,
                    proc: 0,
                    a,
                    b,
                    c,
                },
                None,
            );
        }
        f
    }

    #[test]
    fn a_lazy_value_seen_before_a_failed_flush_may_vanish_but_a_pending_durable_one_may_not() {
        // Durable op 10 sets ref 7 to 0xA and is acknowledged; lazy op 11 sets it to 0xB and is acknowledged; then a
        // flush fails (FM-3.6). The first reader reads the poisoned sector's newest bytes (0xB), the recovering writer
        // its oldest (0xA) — FM-3.2 under the Evict policy — with no crash at all.
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(R, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
            e(3, 0, begin(11, Class::Lazy, &[(R, Some(0xB))])),
            e(4, 0, Rec::Ack { op: 11 }),
        ];
        let r = rec(set(&[(R, 0xB)]), set(&[(R, 0xA)]));
        assert!(
            judge(&log, false, 1, &r).is_empty(),
            "{:?}",
            judge(&log, false, 1, &r)
        );
        // Without the failed flush the lazy effect must survive every process death: the recovered state lost op 11.
        let m = judge(&log, false, 0, &r);
        assert!(
            m.iter()
                .any(|m| m.starts_with("recovered state") && m.contains("op 11")),
            "{m:?}"
        );
        // The lazy effect may also vanish to a clear register.
        let lazy_only = [
            e(1, 0, begin(11, Class::Lazy, &[(R, Some(0xB))])),
            e(2, 0, Rec::Ack { op: 11 }),
        ];
        let r = rec(set(&[(R, 0xB)]), EffectSet::new());
        assert!(judge(&lazy_only, false, 1, &r).is_empty());
        // A failed flush counted between the lazy op's begin and its acknowledgement may have followed its effect.
        let straddling = [
            e(1, 0, begin(11, Class::Lazy, &[(R, Some(0xB))])),
            e(2, 1, Rec::Ack { op: 11 }),
        ];
        assert!(judge(&straddling, false, 1, &r).is_empty());
        // A pending durable value seen and then dropped still fails, failed flush or not.
        let pending = [
            e(1, 0, begin(10, Class::Durable, &[(R, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
            e(3, 0, begin(12, Class::Durable, &[(R, Some(0xC))])),
        ];
        let r = rec(set(&[(R, 0xC)]), set(&[(R, 0xA)]));
        for (crash, ff) in [(false, 1), (true, 1), (true, 0)] {
            let m = judge(&pending, crash, ff, &r);
            assert!(m.iter().any(|m| m.contains("read freshness")), "{m:?}");
        }
    }

    #[test]
    fn after_deaths_alone_the_first_read_may_lag_but_not_invent_and_the_writer_may_not_lag() {
        // Op 10 acknowledged (ref 7 = 0xA), op 11 acknowledged after it (ref 7 = 0xB, commit 2 = 0xB2).
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(R, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
            e(
                3,
                0,
                begin(11, Class::Durable, &[(R, Some(0xB)), (B, Some(0xB2))]),
            ),
            e(4, 0, Rec::Ack { op: 11 }),
        ];
        let full = set(&[(R, 0xB), (B, 0xB2)]);
        // A reader before any writer still sees the older publish (its slot poisoned, or the newer publish's write
        // applied nothing at its writer's death): no failure after deaths alone …
        let lagging = rec(set(&[(R, 0xA)]), full.clone());
        assert!(judge(&log, false, 0, &lagging).is_empty());
        // … but after a crash every read reflects every acknowledged record (I-G2).
        let m = judge(&log, true, 0, &lagging);
        assert!(m.iter().any(|m| m.starts_with("first read")), "{m:?}");
        // The lagging reader never invents a value or shows half an operation.
        let phantom = rec(set(&[(R, 0xC)]), full.clone());
        assert!(
            judge(&log, false, 0, &phantom)
                .iter()
                .any(|m| m.contains("phantom"))
        );
        let half = rec(set(&[(B, 0xB2), (R, 0xA)]), full.clone());
        assert!(
            judge(&log, false, 0, &half)
                .iter()
                .any(|m| m.contains("partly applied"))
        );
        // The writer's recovery may not lag.
        let m = judge(&log, false, 0, &rec(full, set(&[(R, 0xA)])));
        assert!(m.iter().any(|m| m.starts_with("recovered state")), "{m:?}");
    }

    #[test]
    fn a_clear_observation_marks_no_operation_done() {
        // Op 20 releases lease 3 (clears it) and writes its idempotency record; a reader saw the lease clear before op 20
        // ran at all (the lease was never set). Op 20 never acknowledged: its effects are not required.
        let lease = EffectKey::new(EffectKind::Lease, 3);
        let idem = EffectKey::new(EffectKind::Idempotency, 20);
        let log = [
            e(
                1,
                0,
                begin(20, Class::Durable, &[(lease, None), (idem, Some(1))]),
            ),
            e(
                2,
                0,
                Rec::Observe {
                    key: lease,
                    value: None,
                    covered: None,
                },
            ),
        ];
        let none = rec(EffectSet::new(), EffectSet::new());
        let m = judge(&log, true, 0, &none);
        assert!(m.is_empty(), "{m:?}");
        // A value, by contrast, is evidence.
        let log = [
            e(
                1,
                0,
                begin(20, Class::Durable, &[(lease, Some(9)), (idem, Some(1))]),
            ),
            e(
                2,
                0,
                Rec::Observe {
                    key: lease,
                    value: Some(9),
                    covered: None,
                },
            ),
        ];
        assert!(!judge(&log, true, 0, &none).is_empty());
    }

    #[test]
    fn entries_after_the_crash_point_are_not_judged() {
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(A, Some(0xA))])),
            e(5, 0, Rec::Ack { op: 10 }),
        ];
        let t = OpTable::build(&log, 5);
        assert!(
            Verdict::new(&t, true, 0)
                .check(&rec(EffectSet::new(), EffectSet::new()))
                .is_empty()
        );
        let t = OpTable::build(&log, 6);
        assert!(
            !Verdict::new(&t, true, 0)
                .check(&rec(EffectSet::new(), EffectSet::new()))
                .is_empty()
        );
    }

    const FLAG: EffectKey = EffectKey::new(EffectKind::Other(1), 1);

    #[test]
    fn an_unacknowledged_head_only_value_seen_by_a_reader_may_vanish_after_a_failed_flush() {
        // Op 1 set the flag to 0xA by a durable publish, acknowledged after its HEAD flush. Op 2's durable publish sets it
        // to 0xB; a reader saw 0xB; op 2 is not acknowledged (its HEAD flush failed, FM-3.6 "Fields kept only in HEAD").
        let log = [
            e(1, 0, begin_head(1, &[(FLAG, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 1 }),
            e(3, 0, begin_head(2, &[(FLAG, Some(0xB))])),
            e(
                4,
                0,
                Rec::Observe {
                    key: FLAG,
                    value: Some(0xB),
                    covered: None,
                },
            ),
        ];
        // A failed flush counted after op 2 began: reads may alternate (the first reader 0xB, the writer 0xA), and the
        // recovered state may hold either value, with or without a crash.
        for crash in [false, true] {
            let alternate = rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xA)]));
            let m = judge(&log, crash, 1, &alternate);
            assert!(m.is_empty(), "crash {crash}: {m:?}");
            let kept = rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xB)]));
            assert!(judge(&log, crash, 1, &kept).is_empty());
            // The first read after a crash may show the old value too.
            let old = rec(set(&[(FLAG, 0xA)]), set(&[(FLAG, 0xA)]));
            assert!(judge(&log, crash, 1, &old).is_empty());
        }
        // The acknowledged value never vanishes: clear is a lost acknowledged effect.
        let m = judge(&log, true, 1, &rec(EffectSet::new(), EffectSet::new()));
        assert!(m.iter().any(|m| m.contains("op 1")), "{m:?}");
        // Without a failed flush or a crash (deaths alone) the cache keeps the slot writes: the seen value must stay.
        let m = judge(
            &log,
            false,
            0,
            &rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xA)])),
        );
        assert!(
            m.iter()
                .any(|m| m.starts_with("recovered state") && m.contains("op 2")),
            "{m:?}"
        );
        // A crash alone may revert the unflushed slot writes that the reader saw (FM-1.1).
        assert!(
            judge(
                &log,
                true,
                0,
                &rec(set(&[(FLAG, 0xA)]), set(&[(FLAG, 0xA)]))
            )
            .is_empty()
        );
        // A failed flush counted before op 2 began does not follow its slot writes.
        let before: Vec<Entry> = log
            .iter()
            .map(|x| Entry {
                ff: u64::from(x.at >= 3),
                ..x.clone()
            })
            .collect();
        let m = judge(
            &before,
            false,
            1,
            &rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xA)])),
        );
        assert!(m.iter().any(|m| m.contains("op 2")), "{m:?}");
        // Once acknowledged (its HEAD flush returned), op 2 is durable like any other.
        let mut acked = log.to_vec();
        acked.push(e(5, 1, Rec::Ack { op: 2 }));
        for crash in [false, true] {
            let m = judge(
                &acked,
                crash,
                1,
                &rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xA)])),
            );
            assert!(
                m.iter()
                    .any(|m| m.starts_with("recovered state") && m.contains("op 2")),
                "crash {crash}: {m:?}"
            );
        }
    }

    #[test]
    fn the_head_only_allowance_covers_no_other_durable_effect() {
        // The same history with op 2 an ordinary durable operation (a log record): a value a reader saw was flushed
        // (I-G2), so recovery may not drop it, failed flush or crash.
        let log = [
            e(1, 0, begin(1, Class::Durable, &[(FLAG, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 1 }),
            e(3, 0, begin(2, Class::Durable, &[(FLAG, Some(0xB))])),
            e(
                4,
                0,
                Rec::Observe {
                    key: FLAG,
                    value: Some(0xB),
                    covered: None,
                },
            ),
        ];
        for (crash, ff) in [(false, 1), (true, 1), (true, 0)] {
            let m = judge(
                &log,
                crash,
                ff,
                &rec(set(&[(FLAG, 0xB)]), set(&[(FLAG, 0xA)])),
            );
            assert!(
                m.iter()
                    .any(|m| m.starts_with("recovered state") && m.contains("op 2")),
                "{crash} {ff}: {m:?}"
            );
        }
        // A HEAD-only operation and a log record begun under one id are a run problem.
        let t = OpTable::build(
            &[
                e(1, 0, begin_head(7, &[(FLAG, Some(1))])),
                e(2, 0, begin(7, Class::Durable, &[(FLAG, Some(1))])),
            ],
            u64::MAX,
        );
        assert!(
            t.problems.iter().any(|p| p.contains("begun again")),
            "{:?}",
            t.problems
        );
    }

    #[test]
    fn an_unknown_boot_readers_first_read_is_judged_for_consistency_only() {
        // Op 10 acknowledged (ref 7 = 0xA), op 11 acknowledged after it (ref 7 = 0xB, commit 2 = 0xB2), op 12 pending.
        let log = [
            e(1, 0, begin(10, Class::Durable, &[(R, Some(0xA))])),
            e(2, 0, Rec::Ack { op: 10 }),
            e(
                3,
                0,
                begin(11, Class::Durable, &[(R, Some(0xB)), (B, Some(0xB2))]),
            ),
            e(4, 0, Rec::Ack { op: 11 }),
            e(5, 0, begin(12, Class::Durable, &[(A, Some(0xC))])),
        ];
        let full = set(&[(R, 0xB), (B, 0xB2)]);
        // After a crash an Unknown-boot reader may still see the publish before op 11 (a torn slot and a reverted one;
        // the next writer's flush republishes it, U5–U6, P-67) …
        let lagging = rec(set(&[(R, 0xA)]), full.clone());
        assert!(judge_boot(&log, true, 0, BootMode::Unknown, &lagging).is_empty());
        let m = judge_boot(&log, true, 0, BootMode::Known, &lagging);
        assert!(m.iter().any(|m| m.starts_with("first read")), "{m:?}");
        // … but it never invents a value, never shows half an operation, …
        let phantom = rec(set(&[(R, 0xD)]), full.clone());
        let m = judge_boot(&log, true, 0, BootMode::Unknown, &phantom);
        assert!(m.iter().any(|m| m.contains("phantom")), "{m:?}");
        let half = rec(set(&[(B, 0xB2), (R, 0xA)]), full.clone());
        let m = judge_boot(&log, true, 0, BootMode::Unknown, &half);
        assert!(m.iter().any(|m| m.contains("partly applied")), "{m:?}");
        // … never serves a value that recovery then drops (check 3) …
        let dropped = rec(set(&[(R, 0xB), (B, 0xB2), (A, 0xC)]), full.clone());
        let m = judge_boot(&log, true, 0, BootMode::Unknown, &dropped);
        assert!(m.iter().any(|m| m.contains("read freshness")), "{m:?}");
        // … and the recovered state stays complete.
        let m = judge_boot(
            &log,
            true,
            0,
            BootMode::Unknown,
            &rec(full, set(&[(R, 0xA)])),
        );
        assert!(m.iter().any(|m| m.starts_with("recovered state")), "{m:?}");
        // The default is a Known-boot reader.
        let t = OpTable::build(&log, u64::MAX);
        assert_eq!(
            Verdict::new(&t, true, 0).check(&lagging),
            Verdict::new(&t, true, 0)
                .first_read_boot(BootMode::Known)
                .check(&lagging)
        );
    }

    /// Property tests: the verdict never fails a state that a real-time-consistent subset of the operations that took
    /// effect produces when the subset holds every operation a correct store must keep (soundness), and always flags a
    /// required effect replaced by a value no later operation wrote (completeness).
    mod props {
        use std::collections::BTreeMap;

        use proptest::prelude::*;

        use super::super::super::ledger::{EffectKind, EffectWrite, Entry, Rec};
        use super::super::*;
        use crate::rng::Rng;

        /// Property-test cases for the tier `MOIRAI_TEST_TIER` names (PLAN §2.1): `pr` runs `pr` cases, `nightly` 16
        /// times as many, `exit` 256 times as many.
        fn cases(pr: u32) -> u32 {
            match std::env::var("MOIRAI_TEST_TIER").as_deref() {
                Ok("nightly") => pr * 16,
                Ok("exit") => pr * 256,
                _ => pr,
            }
        }

        /// One generated operation.
        struct GenOp {
            id: u64,
            class: Class,
            /// A durable publish of fields kept only in `HEAD`.
            head: bool,
            writes: Vec<EffectWrite>,
            /// Its `begin` entry's index and the failed-flush count then.
            begun: Option<(usize, u64)>,
            effected: bool,
            /// Its acknowledgement's entry index.
            acked: Option<usize>,
            observed: bool,
        }

        /// A generated run: the ledger, the order in which the operations took effect, whether a system crash
        /// followed, and the failed-flush count at the end.
        struct History {
            entries: Vec<Entry>,
            ops: Vec<GenOp>,
            order: Vec<usize>,
            crash: bool,
            ff_end: u64,
        }

        impl History {
            /// A lazy operation's effects, and an unacknowledged `HEAD`-only durable publish's, after a crash or a
            /// failed flush counted after its begin (FM-1, FM-3.6).
            fn may_vanish(&self, i: usize) -> bool {
                let o = &self.ops[i];
                (o.class == Class::Lazy || (o.head && o.acked.is_none()))
                    && (self.crash || o.begun.is_some_and(|(_, ff)| ff < self.ff_end))
            }

            /// A correct store keeps it: it took effect, was acknowledged or seen, and its effects may not vanish.
            fn must(&self, i: usize) -> bool {
                let o = &self.ops[i];
                o.effected && (o.acked.is_some() || o.observed) && !self.may_vanish(i)
            }

            /// The registers after applying the kept operations in the order they took effect.
            fn fold(&self, keep: &[bool]) -> EffectSet {
                let mut s = EffectSet::new();
                for &i in &self.order {
                    if keep[i] {
                        for &(k, v) in &self.ops[i].writes {
                            match v {
                                Some(v) => {
                                    s.insert(k, v);
                                }
                                None => {
                                    s.remove(&k);
                                }
                            }
                        }
                    }
                }
                s
            }
        }

        /// Picks one of `from` (none if it is empty).
        fn one(r: &mut Rng, from: &[usize]) -> Option<usize> {
            (!from.is_empty()).then(|| from[r.below(from.len() as u64) as usize])
        }

        fn history(seed: u64) -> History {
            let mut r = Rng::new(seed);
            let regs = 1 + r.below(4);
            let n = 1 + r.below(7) as usize;
            let collide = r.below(4) == 0;
            let mut ops: Vec<GenOp> = (0..n)
                .map(|i| {
                    let class = if r.below(3) == 0 {
                        Class::Lazy
                    } else {
                        Class::Durable
                    };
                    let head = class == Class::Durable && r.below(3) == 0;
                    let mut writes: Vec<EffectWrite> = Vec::new();
                    for k in 0..regs {
                        if r.below(2) == 0 || (k + 1 == regs && writes.is_empty()) {
                            let v = if r.below(5) == 0 {
                                None
                            } else if collide {
                                Some(1 + r.below(2))
                            } else {
                                Some((i as u64 + 1) * 100 + k)
                            };
                            writes.push((EffectKey::new(EffectKind::Other(0), k), v));
                        }
                    }
                    GenOp {
                        id: 1000 + i as u64,
                        class,
                        head,
                        writes,
                        begun: None,
                        effected: false,
                        acked: None,
                        observed: false,
                    }
                })
                .collect();
            let mut entries = Vec::new();
            let mut order = Vec::new();
            let mut live: BTreeMap<EffectKey, u64> = BTreeMap::new();
            let mut ff = 0u64;
            let steps = 4 * n + r.below(10) as usize;
            for _ in 0..steps {
                let at = entries.len() as u64 + 1;
                match r.below(5) {
                    0 => {
                        let free: Vec<usize> = (0..n).filter(|&i| ops[i].begun.is_none()).collect();
                        if let Some(i) = one(&mut r, &free) {
                            ops[i].begun = Some((entries.len(), ff));
                            entries.push(Entry {
                                at,
                                ff,
                                rec: Rec::Begin {
                                    op: ops[i].id,
                                    class: ops[i].class,
                                    head: ops[i].head,
                                    writes: ops[i].writes.clone(),
                                },
                            });
                        }
                    }
                    1 => {
                        let ready: Vec<usize> = (0..n)
                            .filter(|&i| ops[i].begun.is_some() && !ops[i].effected)
                            .collect();
                        if let Some(i) = one(&mut r, &ready) {
                            ops[i].effected = true;
                            order.push(i);
                            for &(k, v) in &ops[i].writes {
                                match v {
                                    Some(v) => {
                                        live.insert(k, v);
                                    }
                                    None => {
                                        live.remove(&k);
                                    }
                                }
                            }
                        }
                    }
                    2 => {
                        let done: Vec<usize> = (0..n)
                            .filter(|&i| ops[i].effected && ops[i].acked.is_none())
                            .collect();
                        if let Some(i) = one(&mut r, &done) {
                            ops[i].acked = Some(entries.len());
                            entries.push(Entry {
                                at,
                                ff,
                                rec: Rec::Ack { op: ops[i].id },
                            });
                        }
                    }
                    3 => ff += 1,
                    _ => {
                        let key = EffectKey::new(EffectKind::Other(0), r.below(regs));
                        let value = live.get(&key).copied();
                        if value.is_some() {
                            // Every operation that took effect and wrote the value seen might be its source; a correct
                            // store keeps each (a superset of what the verdict requires).
                            for o in ops.iter_mut().filter(|o| o.effected) {
                                if o.writes.contains(&(key, value)) {
                                    o.observed = true;
                                }
                            }
                        }
                        entries.push(Entry {
                            at,
                            ff,
                            rec: Rec::Observe {
                                key,
                                value,
                                covered: None,
                            },
                        });
                    }
                }
            }
            History {
                entries,
                ops,
                order,
                crash: r.below(2) == 0,
                ff_end: ff,
            }
        }

        /// A sound pair of recovered states: the writer's recovery keeps every operation a correct store must keep and
        /// any others that took effect; the first read keeps every one it must, any of the writer's others, and any
        /// whose effects may vanish.
        fn correct_states(h: &History, r: &mut Rng) -> (EffectSet, EffectSet) {
            let (first, state) = correct_keeps(h, r);
            (h.fold(&first), h.fold(&state))
        }

        /// The operations [`correct_states`] keeps in the first read and in the writer's state.
        fn correct_keeps(h: &History, r: &mut Rng) -> (Vec<bool>, Vec<bool>) {
            let n = h.ops.len();
            let state: Vec<bool> = (0..n)
                .map(|i| h.must(i) || (h.ops[i].effected && r.below(2) == 0))
                .collect();
            let first: Vec<bool> = (0..n)
                .map(|i| {
                    h.must(i)
                        || (state[i] && r.below(2) == 0)
                        || (h.ops[i].effected && h.may_vanish(i) && r.below(2) == 0)
                })
                .collect();
            (first, state)
        }

        fn recovered(first: EffectSet, state: EffectSet) -> Recovered {
            Recovered {
                first_read: Ok(first),
                state: Ok(state),
                ..Recovered::same(EffectSet::new())
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: cases(512), failure_persistence: None, ..ProptestConfig::default() })]

            #[test]
            fn a_correct_state_never_fails(seed in any::<u64>(), pick in any::<u64>()) {
                let h = history(seed);
                let t = OpTable::build(&h.entries, u64::MAX);
                prop_assert!(t.problems.is_empty(), "{:?}", t.problems);
                let mut r = Rng::new(pick);
                for _ in 0..4 {
                    let (first, state) = correct_states(&h, &mut r);
                    let m = Verdict::new(&t, h.crash, h.ff_end).check(&recovered(first, state));
                    prop_assert!(m.is_empty(), "{:?}", m);
                }
            }

            /// An Unknown-boot reader's first read after a crash may lag ([F13 §3.8], U5–U6, P-67): any subset of what the
            /// writer's state keeps and of what may vanish, applied in order, passes; the same read by a Known-boot reader
            /// is judged complete.
            #[test]
            fn an_unknown_boot_first_read_may_lag_behind_a_correct_state(seed in any::<u64>(), pick in any::<u64>()) {
                // The boot mode matters after a system crash only.
                let mut h = history(seed);
                h.crash = true;
                let t = OpTable::build(&h.entries, u64::MAX);
                let mut r = Rng::new(pick);
                for _ in 0..4 {
                    let (_, state) = correct_keeps(&h, &mut r);
                    let lagging: Vec<bool> = (0..h.ops.len())
                        .map(|i| (state[i] || (h.ops[i].effected && h.may_vanish(i))) && r.below(2) == 0)
                        .collect();
                    let first = h.fold(&lagging);
                    let rec = recovered(first.clone(), h.fold(&state));
                    let m = Verdict::new(&t, h.crash, h.ff_end).first_read_boot(BootMode::Unknown).check(&rec);
                    prop_assert!(m.is_empty(), "{:?}", m);
                    // A first read with a clear register that a required operation sets, and that no operation clears,
                    // fails when a Known-boot reader made it.
                    let never_cleared = |k: &EffectKey| h.ops.iter().all(|o| !o.writes.contains(&(*k, None)));
                    let misses = (0..h.ops.len()).any(|i| {
                        h.must(i)
                            && h.ops[i].acked.is_some()
                            && h.ops[i]
                                .writes
                                .iter()
                                .any(|(k, v)| v.is_some() && never_cleared(k) && !first.contains_key(k))
                    });
                    if misses {
                        let known = Verdict::new(&t, h.crash, h.ff_end).check(&rec);
                        prop_assert!(!known.is_empty(), "a Known-boot first read missing a required effect passed");
                    }
                }
            }

            #[test]
            fn a_replaced_required_effect_is_always_flagged(seed in any::<u64>(), pick in any::<u64>()) {
                let h = history(seed);
                let t = OpTable::build(&h.entries, u64::MAX);
                let mut r = Rng::new(pick);
                let (first, state) = correct_states(&h, &mut r);
                for (ri, ro) in h.ops.iter().enumerate() {
                    // Acknowledged durable operations are required whatever the verdict's other rules say.
                    let (Some((rb, _)), Some(_), Class::Durable) = (ro.begun, ro.acked, ro.class) else {
                        continue;
                    };
                    for &(k, v) in &ro.writes {
                        // Only where no other writer of the register may follow it: each was acknowledged before it
                        // began.
                        let others: Vec<&GenOp> = h
                            .ops
                            .iter()
                            .enumerate()
                            .filter(|&(j, o)| j != ri && o.writes.iter().any(|w| w.0 == k))
                            .map(|(_, o)| o)
                            .collect();
                        if !others.iter().all(|o| o.acked.is_some_and(|a| a < rb)) {
                            continue;
                        }
                        prop_assert_eq!(state.get(&k).copied(), v, "the writer's state keeps op {}", ro.id);
                        let mut wrong: Vec<Option<u64>> = vec![None, Some(0xDEAD_0000 + k.key)];
                        wrong.extend(others.iter().flat_map(|o| o.writes.iter()).filter(|w| w.0 == k).map(|w| w.1));
                        for w in wrong.into_iter().filter(|&w| w != v) {
                            let mut bad = state.clone();
                            match w {
                                Some(x) => {
                                    bad.insert(k, x);
                                }
                                None => {
                                    bad.remove(&k);
                                }
                            }
                            let m = Verdict::new(&t, h.crash, h.ff_end).check(&recovered(first.clone(), bad));
                            prop_assert!(!m.is_empty(), "op {} lost {:?} to {:?} unnoticed", ro.id, k, w);
                        }
                    }
                }
            }
        }
    }
}
