//! Recovery ([F16 §10]): boot-change recovery (P-66) with its scan start (P-64), the adoption of groups beyond the
//! published `committed_lsn` by a flush holder's pass (P-65), and the recovering writer's steps that a test runs after a
//! crash or after process deaths.

use moirai_vfs::{BootIdentity, Vfs};

use crate::bugs::Bug;
use crate::ops::Op;
use crate::store::{ScanCtx, Stop, Toy, ToyError};
use crate::tap::Tap;
use crate::write::{Change, Held, activity};

impl<V: Vfs, T: Tap> Toy<V, T> {
    /// Boot-change recovery ([F16] P-66): flush byte, writer byte; if another process has not recovered already, scan
    /// from P-64's start to the end of the valid log, re-write every byte of `(durable_lsn, E_v]`, release the writer
    /// byte and flush it, then make a durable publish whose first write sets `boot_id`. A process in Unknown-boot mode
    /// never runs it (P-67, U2).
    ///
    /// Seeded bugs: P-64 scans from `committed_lsn`; P-66 skips the re-write; P-15 publishes before the flush.
    pub(crate) fn boot_recover(&mut self) -> Result<(), ToyError> {
        let BootIdentity::Known(b) = self.boot else {
            return Ok(());
        };
        let bugs = self.bugs();
        let mut held = Held::default();
        if !self.take_flush()? {
            return Err(ToyError::StoreLocked);
        }
        held.flush = true;
        let r = (|| {
            held.writer = self.take_writer(activity::BOOT_RECOVERY)?;
            let (s, _) = self.read_head()?;
            if s.boot_id == b.0 {
                return Ok(());
            }
            let ctx = ScanCtx::of(&s);
            let start = if bugs.on(Bug::P64T7ScanFromCommitted) {
                s.committed_lsn
            } else {
                s.checkpoint_lsn.min(s.durable_lsn)
            };
            let chain = self.chain_at(&ctx, start, s.durable_lsn)?;
            let sc = self.scan_opt(&ctx, start, chain, None, true)?;
            match sc.stop {
                Stop::ReadError(p) => {
                    return Err(if p < s.durable_lsn {
                        ToyError::Corrupt(format!("unreadable log at {p} below durable_lsn"))
                    } else {
                        ToyError::IoFault(p)
                    });
                }
                Stop::Invalid(_, p) | Stop::NoExtent(p) if p < s.durable_lsn => {
                    return Err(ToyError::Corrupt(format!(
                        "an invalid group at {p} below durable_lsn; run moirai repair"
                    )));
                }
                _ => {}
            }
            let e_v = sc.end;
            if !bugs.on(Bug::P66BootRecoveryWithoutRewrite) {
                for g in sc.groups.iter().filter(|g| g.end > s.durable_lsn) {
                    self.write_log(&ctx, g.start, &g.raw)?;
                }
            }
            if held.writer {
                self.drop_writer();
                held.writer = false;
            }
            let change = Change {
                boot: Some(b.0),
                flushed: true,
                ..Change::default()
            };
            let d = s.durable_lsn.max(e_v);
            if bugs.on(Bug::P15BootRecoveryPublishBeforeFlush) {
                self.durable_publish(activity::BOOT_RECOVERY, Some(d), change)?;
                self.flush_range(&ctx, s.durable_lsn, e_v)?;
            } else {
                self.flush_range(&ctx, s.durable_lsn, e_v)?;
                self.durable_publish(activity::BOOT_RECOVERY, Some(d), change)?;
            }
            Ok(())
        })();
        self.let_go(&mut held);
        r
    }

    /// A flush holder's pass with no group of its own ([F16] P-42–P-45, P-65): every group beyond `durable_lsn` is
    /// re-written, flushed and published — the adoption of a dead writer's pending groups.
    pub fn adopt(&mut self) -> Result<(), ToyError> {
        let bugs = self.bugs();
        let mut held = Held::default();
        if !self.take_flush()? {
            return Err(ToyError::StoreLocked);
        }
        held.flush = true;
        let r = (|| {
            held.writer = self.take_writer(activity::FLUSH_SCAN)?;
            let (s, _) = self.read_head()?;
            let ctx = ScanCtx::of(&s);
            let chain = self.chain_at(&ctx, s.durable_lsn, s.durable_lsn)?;
            let sc = self.scan_opt(&ctx, s.durable_lsn, chain, None, true)?;
            match sc.stop {
                Stop::ReadError(p) => return Err(ToyError::IoFault(p)),
                Stop::Invalid(_, p) | Stop::NoExtent(p) if p < s.durable_lsn => {
                    return Err(ToyError::Corrupt(format!(
                        "an invalid group at {p} below durable_lsn; run moirai repair"
                    )));
                }
                _ => {}
            }
            let e_end = sc.end;
            if e_end <= s.durable_lsn && s.committed_lsn == e_end {
                return Ok(());
            }
            if !bugs.on(Bug::P42G3T5FlushWithoutRewrite) {
                for g in &sc.groups {
                    self.write_log(&ctx, g.start, &g.raw)?;
                }
            }
            if held.writer && !bugs.on(Bug::P02FlushUnderWriter) {
                self.drop_writer();
                held.writer = false;
            }
            self.flush_range(&ctx, s.durable_lsn, e_end)?;
            if !held.writer {
                held.writer = self.take_writer(activity::FLUSH_PUBLISH)?;
            }
            // P-45: durable_lsn never decreases.
            let (s2, w2) = self.read_head()?;
            let d = s2.durable_lsn.max(e_end);
            self.publish(
                &s2,
                w2,
                d,
                Change {
                    flushed: true,
                    ..Change::default()
                },
            )?;
            Ok(())
        })();
        self.let_go(&mut held);
        r
    }

    /// The recovering writer after a crash or after process deaths: the boot check (P-60, P-66), the adoption of pending
    /// groups, intent recovery (P-71), and one durable probe write on the toy's hidden probe ref (which parks a failing
    /// commit first, P-70), showing that the store accepts writes. `probe` is the probe's operation id.
    pub fn recover_writer(&mut self, probe: u64) -> Result<(), ToyError> {
        self.head_for_read()?;
        self.adopt()?;
        self.recover_intents()?;
        self.run(&Op::Probe(probe))?;
        Ok(())
    }
}

/// Recovery on the in-memory `Vfs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ops::{CommitOp, RuntimeOp};
    use crate::state::MAIN;
    use crate::testing::{open, sim_store};
    use moirai_vfs_sim::CrashPlan;

    fn commit(op: u64) -> Op {
        Op::Commit(CommitOp {
            op,
            digest: op,
            ref_name: MAIN,
            creates: vec![op << 8],
            key: Some(op),
            filler: 200,
            ..CommitOp::default()
        })
    }

    #[test]
    fn a_new_boot_runs_boot_change_recovery_before_the_first_read() {
        let c = Config::test_profile();
        let (w, v, _) = sim_store(&c, 41);
        let mut t = open(&v, &c);
        let d = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        drop(t);
        // A system crash: the unflushed publish of the commit is lost, the flushed log is not.
        w.crash(&CrashPlan::baseline())
            .unwrap_or_else(|e| panic!("{e:?}"));
        let v2 = w.process_with("after", None, Some(true));
        let mut t2 = open(&v2, &c);
        let s = t2.head_for_read().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.boot_id, w.boot().1.0, "the slot carries the new boot");
        assert!(s.durable_lsn >= d.end && s.committed_lsn >= d.end);
        let st = t2.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1));
        // Both slots carry it (a durable publish, [F16] P-13).
        let other = t2.other_slot().unwrap_or_else(|| panic!("two valid slots"));
        assert_eq!(other.boot_id, s.boot_id);
    }

    /// [F16] P-60: a boot-change recovery that refuses leaves the handle unbooted. Here another process holds the flush
    /// byte (as a dead process's byte does until its release, [F15] FM-8.1), so the recovery ends `store_locked`; the
    /// next read on the same handle runs the boot check again and refuses again instead of replaying the slot of the
    /// earlier boot, and once the byte is free the recovery runs and the read follows it.
    #[test]
    fn a_refused_boot_change_recovery_leaves_the_handle_unbooted() {
        let c = Config::test_profile();
        let (w, v, _) = sim_store(&c, 43);
        let mut t = open(&v, &c);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        drop(t);
        w.crash(&CrashPlan::baseline())
            .unwrap_or_else(|e| panic!("{e:?}"));
        let holder = w.process_with("holder", None, Some(true));
        let mut u = open(&holder, &c);
        assert_eq!(u.take_flush(), Ok(true));
        let v2 = w.process_with("after", None, Some(true));
        let mut t2 = open(&v2, &c);
        assert_eq!(t2.refresh().map(|s| s.slot_seq), Err(ToyError::StoreLocked));
        assert_eq!(
            t2.refresh().map(|s| s.slot_seq),
            Err(ToyError::StoreLocked),
            "the second read skipped the boot check"
        );
        assert!(t2.view().is_none());
        u.drop_flush();
        let s = t2.refresh().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.boot_id, w.boot().1.0, "the slot carries the new boot");
        let st = t2.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1));
    }

    #[test]
    fn adoption_flushes_and_publishes_every_group_beyond_durable_lsn() {
        let c = Config::test_profile();
        let (_w, v, _) = sim_store(&c, 42);
        let mut t = open(&v, &c);
        // A lazy batch is published at once, not flushed.
        t.run(&Op::Runtime(RuntimeOp {
            op: 3,
            rows: vec![(3, 3)],
            pad: 100,
            symbols: Vec::new(),
            target_len: 0,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert!(s.durable_lsn < s.committed_lsn);
        t.adopt().unwrap_or_else(|e| panic!("{e}"));
        let (s2, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s2.durable_lsn, s.committed_lsn);
        assert_eq!(s2.committed_lsn, s.committed_lsn);
        // Nothing beyond durable_lsn: adoption publishes nothing.
        t.adopt().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            t.read_head().unwrap_or_else(|e| panic!("{e}")).0.slot_seq,
            s2.slot_seq
        );
    }
}
