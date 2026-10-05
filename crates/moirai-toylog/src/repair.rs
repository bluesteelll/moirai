//! `repair` of a store with no valid `HEAD` slot ([F16] P-85, [F04 §8.1], [F15] OP-1): a failed `HEAD` flush can leave
//! both slots failing validation, and every process then exits 7 naming `moirai repair`. A fatal slot ([F04 §7]) is
//! repaired the same way: plain `repair` trusts neither slot (spec sync 2b S2B-P-28). The slot state is rebuilt from
//! the log's extent heads ([F05 §4.5], §9.28): the head of the greatest extent that validates by itself gives the epoch;
//! the log is scanned by the chain rule from the lowest extent of that epoch from which every later extent exists, and
//! every scanned group is re-written with the bytes the scan validated before the flush (a failed flush may have
//! poisoned them, [F15] FM-3.4, FM-3.5); the counters, `init` and flags come from the heads, and the fold of every
//! scanned group gives the rest.

use moirai_vfs::{LockByte, SyncKind, Vfs};

use crate::codec::{hash64_seeded, u64_at};
use crate::format::{
    ExtentHeadRec, HEAD_GROUP, RECHDR, TRAILER, kind, peek_header, validate_record,
};
use crate::head::{FLAG_QUIET, FLAG_READONLY, HEAD_LEN, ImageCursor, SLOT_LEN, Slot, SlotRead};
use crate::state::fold_slot;
use crate::store::{HEAD_UNREADABLE, ScanCtx, Stop, Toy, ToyError};
use crate::tap::Tap;

/// The most bytes of the log a `repair` scans and re-writes before it reads on ([`Toy::rewrite_from`]): 1 MiB, or
/// E / 16 when that is less (4 KiB at the test profile's E, so the unit tests go through many chunks).
const REWRITE_CHUNK: u64 = 1 << 20;

/// An extent head that validated by itself: its extent, epoch and payload.
#[derive(Clone, Debug)]
struct Head {
    n: u32,
    epoch: u64,
    rec: ExtentHeadRec,
}

impl<V: Vfs, T: Tap> Toy<V, T> {
    /// Reads the extent head of `log.<n>` and validates it by itself: its record checksum, its position (`lsn` =
    /// (n − 1)·E with the E its own `init` carries, [F16] P-54), its epoch ([F16] P-55: `epoch` when given, which is step
    /// 2's "of that epoch", else the one it carries) and its trailer recomputed with the `chain_in` it carries.
    ///
    /// `Ok(None)`: `log.<n>` does not exist, is shorter than a head, or its head does not validate. A failed open or read
    /// is an error, never a head that does not validate: taken for one, it would let a lower extent of another epoch, or
    /// a misplaced copy, be chosen in its place (P-85 steps 1 and 2 with P-92: a failed read stops `repair` with exit 7).
    fn lone_head(&mut self, n: u32, epoch: Option<u64>) -> Result<Option<Head>, ToyError> {
        if !self.probe_extent(n)? {
            return Ok(None);
        }
        let Some(f) = self.ext(n) else {
            return Ok(None);
        };
        if self.vfs.file_size(f).map_err(ToyError::Io)? < HEAD_GROUP {
            return Ok(None);
        }
        let mut b = vec![0u8; HEAD_GROUP as usize];
        if self.vfs.read_exact_at(f, 0, &mut b).is_err() {
            return Err(ToyError::IoFault(u64::from(n - 1) * self.cfg.extent_bytes));
        }
        Ok(self.head_of(n, epoch, &b))
    }

    /// The extent head in the bytes `b` of `log.<n>`'s first group, if it validates by itself ([`Toy::lone_head`]).
    fn head_of(&self, n: u32, epoch: Option<u64>, b: &[u8]) -> Option<Head> {
        let (len, k, _) = peek_header(b)?;
        if u64::from(len) != HEAD_GROUP || k != kind::EXTENT_HEAD {
            return None;
        }
        let rec = ExtentHeadRec::decode(&b[RECHDR..HEAD_GROUP as usize - TRAILER]).ok()?;
        let e = rec.init.log_extent_bytes;
        if !rec.init.valid() {
            return None;
        }
        let lsn = u64::from(n - 1) * e;
        let own = u64_at(b, 16)?;
        validate_record(b, 0, e, lsn, epoch.unwrap_or(own), self.bugs()).ok()?;
        let trailer = u64_at(b, HEAD_GROUP as usize - TRAILER)?;
        (hash64_seeded(&b[..HEAD_GROUP as usize - TRAILER], rec.chain_in) == trailer)
            .then_some(Head { n, epoch: own, rec })
    }

    /// P-85's plain `repair` of a store with no valid `HEAD` slot: under the maintenance and flush bytes (the maintenance
    /// byte by try: `Busy` is `maintenance_busy`), rebuild the slot state from the extent heads and the log, flush every
    /// extent scanned, write both slots under the writer byte, and flush `HEAD` after releasing it.
    pub fn repair_head(&mut self) -> Result<(), ToyError> {
        if !self
            .locks
            .try_take(&self.vfs, LockByte::Maintenance)
            .map_err(|e| ToyError::Lock(e.to_string()))?
        {
            return Err(ToyError::Busy);
        }
        let r = (|| {
            if !self.take_flush()? {
                return Err(ToyError::StoreLocked);
            }
            let r = self.repair_head_held();
            self.drop_flush();
            r
        })();
        self.locks.release(&self.vfs, LockByte::Maintenance);
        r
    }

    fn repair_head_held(&mut self) -> Result<(), ToyError> {
        // The slots are rebuilt even when a read of HEAD now validates: a sector a failed flush poisoned returns a fresh
        // draw from its candidates on every read (FM-3.2), so one valid read proves nothing until the slots are written
        // again. The rebuilt state comes from the log, so a repair after another one writes the same state.
        let entries = self.vfs.list_dir(&self.root, None).map_err(ToyError::Io)?;
        let mut numbers: Vec<u32> = entries
            .iter()
            .filter_map(|e| e.name.as_segment()?.strip_prefix("log.")?.parse().ok())
            .collect();
        numbers.sort_unstable();
        // Step 1: the head of the greatest extent that validates by itself gives the epoch.
        let mut found = None;
        for &n in numbers.iter().rev() {
            if let Some(h) = self.lone_head(n, None)? {
                found = Some(h);
                break;
            }
        }
        let Some(newest) = found else {
            return Err(ToyError::Corrupt(
                "no extent head validates; the store cannot be repaired".to_owned(),
            ));
        };
        let epoch = newest.epoch;
        let epoch_lsn = newest.rec.epoch_lsn;
        let e = newest.rec.init.log_extent_bytes;
        let first_of_epoch = (epoch_lsn / e + 1) as u32;
        // Step 2: the lowest extent of the epoch from which every later extent exists, with a head that validates by
        // itself in that epoch. Its head is the one scan start whose chain value comes from the record itself instead of
        // `HEAD` (P-97), so only the head's own position and epoch checks (P-54, P-55) keep a misplaced or foreign
        // extent out of the rebuilt state.
        let mut lowest = newest.clone();
        let mut n = newest.n;
        while n > first_of_epoch && numbers.contains(&(n - 1)) {
            match self.lone_head(n - 1, Some(epoch))? {
                Some(h) => {
                    lowest = h;
                    n -= 1;
                }
                None => break,
            }
        }
        let ctx = ScanCtx {
            e,
            epoch,
            epoch_lsn,
            init: newest.rec.init,
            algo: newest.rec.project_oid_algo,
        };
        let start = ctx.start_of(lowest.n);
        // Step 3's slot state, from the heads; the scan below folds every group it reads into it.
        let mut flags = 0u16;
        if newest.rec.hflags & 1 != 0 {
            flags |= FLAG_QUIET;
        }
        if newest.rec.hflags & 2 != 0 {
            flags |= FLAG_READONLY;
        }
        let mut s = Slot {
            format: 1,
            flags,
            slot_seq: 1,
            epoch,
            committed_lsn: start,
            durable_lsn: start,
            boot_id: match self.boot {
                moirai_vfs::BootIdentity::Known(b) => b.0,
                moirai_vfs::BootIdentity::Unknown(_) => [0; 16],
            },
            config_gen: 0,
            checkpoint_lsn: if lowest.n == first_of_epoch {
                epoch_lsn
            } else {
                start
            },
            counters: lowest.rec.counters,
            active_log: lowest.n,
            segments: Vec::new(),
            refs_lsn: 0,
            pins_lsn: 0,
            heads_lsn: 0,
            markers_lsn: 0,
            image_cursor: [ImageCursor::EMPTY; 4],
            seq_ring: [(0, 0); 32],
            init: newest.rec.init,
            epoch_lsn,
            project_oid_algo: newest.rec.project_oid_algo,
        };
        // Step 4's flush first, after a re-write of every scanned group with the bytes the scan validated ([80 §2.3.4]
        // decision (a), "re-write, then flush"): no slot says which sectors a failed flush poisoned, a flush alone proves
        // nothing about a poisoned sector ([F15] FM-3.4), and a later read of one may return another version (FM-3.2)
        // below the `durable_lsn` this repair publishes; the re-write makes every scanned byte definite (FM-3.5). [F16]
        // P-85 step 4 names only the flush (WP-40 spec finding). Like every flush holder's, the scan and the re-write are
        // made under the writer byte, and the flush outside it ([F13 §3.8] I-G4, [F16] P-2, P-43): a writer whose read of
        // a poisoned `HEAD` drew a valid slot may append meanwhile. The re-write goes chunk by chunk
        // ([`Toy::rewrite_from`]).
        let held = self.take_writer(crate::write::activity::MAINTENANCE)?;
        let r = self.rewrite_from(&ctx, start, lowest.rec.chain_in, &mut s);
        if held {
            self.drop_writer();
        }
        let (end, chain) = r?;
        self.flush_range(&ctx, start, end)?;
        s.committed_lsn = end;
        s.durable_lsn = end;
        // Step 4: both slots under the writer byte, then the HEAD flush outside it (a durable publish whose source state
        // comes from the log). A writer whose read of the poisoned HEAD drew a valid slot may have appended since the
        // scan: its groups are scanned under the writer byte and published by P-49 — lazy groups up to the first durable
        // one, which this repair did not flush (its writer's own flush covers it later).
        let held = self.take_writer(crate::write::activity::MAINTENANCE)?;
        let r = (|| {
            let more = self.scan(&ctx, end, chain, None)?;
            if let Stop::ReadError(p) = more.stop {
                return Err(ToyError::IoFault(p));
            }
            let mut committed = end;
            for g in more.groups.iter().take_while(|g| !g.durable()) {
                fold_slot(&mut s, g, self.bugs(), false)?;
                committed = g.end;
            }
            s.committed_lsn = committed;
            // Two writes, one slot each, as the two publishes of a durable publish ([F04 §9.2]): a death or a failed
            // write cuts at most one of them ([F15] FM-5.2, §2.5). The slot that now reads as valid, if only one does,
            // is replaced first: a cut first write then leaves the other slot — fatal or invalid, the refusal that sent
            // the operator here — in place, never a stale valid slot that a destroyed fatal one had kept readers from
            // (P-61; WP-40 spec finding). The order needs a read of both slots, made like every read of `HEAD`, at most
            // three times ([F04 §8.1]): when every read fails, which slot is valid is unknown, and the repair writes
            // nothing and exits 7 `store_io_fault` ([F16] P-92, [F15] FM-12.2), as its scan does after a failed read.
            let mut cur = [0u8; HEAD_LEN];
            if !(0..3).any(|_| matches!(self.read_slots(&mut cur), Ok(n) if n == HEAD_LEN)) {
                return Err(ToyError::IoFault(HEAD_UNREADABLE));
            }
            let valid = |k: usize| {
                matches!(
                    Slot::read(&cur[k * SLOT_LEN..(k + 1) * SLOT_LEN]),
                    SlotRead::Valid(_)
                )
            };
            let first = usize::from(valid(1) && !valid(0));
            for (k, seq) in [(first, 1), (1 - first, 2)] {
                s.slot_seq = seq;
                let w = self
                    .vfs
                    .write_at(&self.head, (k * SLOT_LEN) as u64, &s.to_bytes());
                self.io(w)?;
            }
            Ok(())
        })();
        if held {
            self.drop_writer();
        }
        r?;
        self.sync_file(&self.head, SyncKind::DataAndMeta);
        self.view = None;
        Ok(())
    }

    /// Step 4's re-write ([`Toy::repair_head`]): scans the log from the group boundary `from`, whose chain value is
    /// `chain`, to the end of the valid log by the chain rule, re-writes every group it read with the bytes it validated
    /// and folds it into `s`. The scan goes a chunk of at most [`REWRITE_CHUNK`] bytes at a time and re-writes each chunk
    /// before it reads on, so the repair holds one chunk of groups (one group, if that is longer), never the whole
    /// active log. Returns the end of the valid log
    /// and the chain value there.
    fn rewrite_from(
        &mut self,
        ctx: &ScanCtx,
        from: u64,
        chain: u64,
        s: &mut Slot,
    ) -> Result<(u64, u64), ToyError> {
        let bugs = self.bugs();
        let (mut p, mut chain) = (from, chain);
        let chunk = REWRITE_CHUNK.min(ctx.e / 16);
        let mut span = chunk;
        loop {
            let sc = self.scan_opt(ctx, p, chain, Some(p.saturating_add(span)), true)?;
            if let Stop::ReadError(q) = sc.stop {
                return Err(ToyError::IoFault(q));
            }
            for g in &sc.groups {
                self.write_log(ctx, g.start, &g.raw)?;
                fold_slot(s, g, bugs, false)?;
            }
            if sc.stop != Stop::Limit {
                return Ok((sc.end, sc.chain));
            }
            if sc.groups.is_empty() {
                // The next group is longer than the chunk. A group never spans extents ([F05 §4.4] G-3), so a limit one
                // extent away admits it; a scan with that limit that reads no group and stops at it cannot happen.
                if span >= ctx.e {
                    return Err(ToyError::Corrupt(format!(
                        "the scan from {p} limited to one extent read no group"
                    )));
                }
                span = ctx.e;
                continue;
            }
            span = chunk;
            (p, chain) = (sc.end, sc.chain);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use moirai_vfs::StoreFs;
    use moirai_vfs_sim::{SimVfs, Site};

    use super::Head;
    use crate::head::HEAD_LEN;
    use crate::ops::{CommitOp, Op};
    use crate::state::MAIN;
    use crate::store::{Toy, ToyError};
    use crate::testing::{EPOCH, STORE, open, sim_store};
    use crate::{Bug, Bugs, Config};

    fn head(t: &mut Toy<SimVfs>, n: u32, epoch: Option<u64>) -> Option<Head> {
        t.lone_head(n, epoch).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [F16] P-85 steps 1 and 2 with P-92: a failed read of an extent head stops `repair` (exit 7 `store_io_fault`)
    /// instead of counting as a head that does not validate, which could let a lower extent of another epoch or a
    /// misplaced copy be chosen in its place; the operator's next `repair` rebuilds the store.
    #[test]
    fn a_failed_read_of_an_extent_head_stops_repair() {
        let cfg = Config::test_profile();
        let (w, v, _) = sim_store(&cfg, 34);
        let mut t = open(&v, &cfg);
        let commit = Op::Commit(CommitOp {
            op: 1,
            digest: 1,
            ref_name: MAIN,
            creates: vec![1 << 8],
            key: Some(1),
            filler: 200,
            ..CommitOp::default()
        });
        t.run(&commit).unwrap_or_else(|e| panic!("{e}"));
        t.vfs
            .write_at(&t.head, 0, &vec![0xA5; HEAD_LEN])
            .unwrap_or_else(|e| panic!("{e}"));
        let log1 = w
            .node_at(&Path::new(STORE).join("log.1"))
            .unwrap_or_else(|| panic!("log.1"));
        w.queue_choice_on(None, Site::ReadFault, log1, 1);
        assert!(matches!(t.repair_head(), Err(ToyError::IoFault(_))));
        assert_eq!(t.read_head().map(|_| ()), Err(ToyError::NoValidSlot));
        t.repair_head().unwrap_or_else(|e| panic!("{e}"));
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1));
    }

    /// [F16] P-85 step 4: the re-write goes chunk by chunk over a log of several extents, with groups longer than a
    /// chunk among them, and the rebuilt slots cover every group of it.
    #[test]
    fn repair_rewrites_a_log_of_several_extents_chunk_by_chunk() {
        let cfg = Config::test_profile();
        let (_w, v, _) = sim_store(&cfg, 35);
        let mut t = open(&v, &cfg);
        for op in 1..=30u64 {
            let c = Op::Commit(CommitOp {
                op,
                digest: op,
                ref_name: MAIN,
                creates: vec![op << 8],
                key: Some(op),
                filler: if op % 3 == 0 { 4_000 } else { 1_500 },
                ..CommitOp::default()
            });
            t.run(&c).unwrap_or_else(|e| panic!("{e}"));
        }
        let (before, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert!(
            before.committed_lsn > cfg.extent_bytes,
            "the log spans extents"
        );
        t.vfs
            .write_at(&t.head, 0, &vec![0xA5; HEAD_LEN])
            .unwrap_or_else(|e| panic!("{e}"));
        t.repair_head().unwrap_or_else(|e| panic!("{e}"));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            (s.committed_lsn, s.durable_lsn),
            (before.committed_lsn, before.committed_lsn)
        );
        assert_eq!(s.counters.commit_seq, before.counters.commit_seq);
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!((1..=30).all(|o| st.commits.contains_key(&o)));
    }

    /// [F16] P-54 and P-55 on the scan start that `repair` seeds from the record itself (P-85 step 2, P-97): a head
    /// validates by itself only at its own position and, in step 2, only in the epoch it is asked for; each seeded bug
    /// drops exactly its check.
    #[test]
    fn a_lone_head_validates_only_at_its_position_and_in_its_epoch() {
        let cfg = Config::test_profile();
        let (w, v, img) = sim_store(&cfg, 1);
        // A byte copy of log.1 as log.2: its head carries lsn 0 at position E.
        let mut copy = vec![0u8; cfg.extent_bytes as usize];
        copy[..img.log.len()].copy_from_slice(&img.log);
        w.put_file(&Path::new(STORE).join("log.2"), &copy)
            .unwrap_or_else(|e| panic!("log.2: {e:?}"));
        let mut t = open(&v, &cfg);
        let h = head(&mut t, 1, None).map(|h| (h.n, h.epoch));
        assert_eq!(h, Some((1, EPOCH)));
        assert!(head(&mut t, 1, Some(EPOCH)).is_some());
        assert!(head(&mut t, 1, Some(EPOCH ^ 1)).is_none(), "another epoch");
        assert!(head(&mut t, 2, None).is_none(), "another position");
        assert!(head(&mut t, 3, None).is_none(), "no log.3");
        let mut p54 = open(
            &v,
            &cfg.clone()
                .with_bugs(Bugs::only(Bug::P54T13NoPositionCheck)),
        );
        assert_eq!(head(&mut p54, 2, None).map(|h| h.n), Some(2));
        assert!(head(&mut p54, 1, Some(EPOCH ^ 1)).is_none());
        let mut p55 = open(&v, &cfg.with_bugs(Bugs::only(Bug::P55NoEpochCheck)));
        let h = head(&mut p55, 1, Some(EPOCH ^ 1)).map(|h| h.epoch);
        assert_eq!(h, Some(EPOCH), "the head keeps the epoch it carries");
        assert!(head(&mut p55, 2, None).is_none());
    }
}
