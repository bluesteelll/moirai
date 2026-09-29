//! `repair` of a store with no valid `HEAD` slot ([F16] P-85, [F04 §8.1], [F15] OP-1): a failed `HEAD` flush can leave
//! both slots failing validation, and every process then exits 7 naming `moirai repair`. The slot state is rebuilt from
//! the log's extent heads ([F05 §4.5], §9.28): the head of the greatest extent that validates by itself gives the epoch;
//! the log is scanned by the chain rule from the lowest extent of that epoch from which every later extent exists; the
//! counters, `init` and flags come from the heads, and the fold of every scanned group gives the rest.

use moirai_vfs::{LockByte, SyncKind, Vfs};

use crate::codec::{hash64_seeded, u64_at};
use crate::format::{
    ExtentHeadRec, HEAD_GROUP, RECHDR, TRAILER, kind, peek_header, validate_record,
};
use crate::head::{FLAG_QUIET, FLAG_READONLY, HEAD_LEN, SLOT_LEN, Slot};
use crate::state::fold_slot;
use crate::store::{ScanCtx, Stop, Toy, ToyError};
use crate::tap::Tap;

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
    fn lone_head(&mut self, n: u32, epoch: Option<u64>) -> Option<Head> {
        if !self.open_extent(n).ok()? {
            return None;
        }
        let f = self.ext(n)?;
        let mut b = vec![0u8; HEAD_GROUP as usize];
        self.vfs.read_exact_at(f, 0, &mut b).ok()?;
        let (len, k, _) = peek_header(&b)?;
        if u64::from(len) != HEAD_GROUP || k != kind::EXTENT_HEAD {
            return None;
        }
        let rec = ExtentHeadRec::decode(&b[RECHDR..HEAD_GROUP as usize - TRAILER]).ok()?;
        let e = rec.init.log_extent_bytes;
        if !rec.init.valid() {
            return None;
        }
        let lsn = u64::from(n - 1) * e;
        let own = u64_at(&b, 16)?;
        validate_record(&b, 0, e, lsn, epoch.unwrap_or(own), self.bugs()).ok()?;
        let trailer = u64_at(&b, HEAD_GROUP as usize - TRAILER)?;
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
        let Some(newest) = numbers.iter().rev().find_map(|&n| self.lone_head(n, None)) else {
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
            match self.lone_head(n - 1, Some(epoch)) {
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
        let sc = self.scan(&ctx, start, lowest.rec.chain_in, None)?;
        if let Stop::ReadError(p) = sc.stop {
            return Err(ToyError::IoFault(p));
        }
        // Step 4's flush first: every extent scanned.
        self.flush_range(&ctx, start, sc.end)?;
        // Step 3: the slot state.
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
            committed_lsn: sc.end,
            durable_lsn: sc.end,
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
            seq_ring: [(0, 0); 32],
            init: newest.rec.init,
            epoch_lsn,
            project_oid_algo: newest.rec.project_oid_algo,
        };
        for g in &sc.groups {
            fold_slot(&mut s, g, self.bugs(), false)?;
        }
        // Step 4: both slots under the writer byte, then the HEAD flush outside it (a durable publish whose source state
        // comes from the log). A writer whose read of the poisoned HEAD drew a valid slot may have appended since the
        // scan: its groups are scanned under the writer byte and published by P-49 — lazy groups up to the first durable
        // one, which this repair did not flush (its writer's own flush covers it later).
        let held = self.take_writer(crate::write::activity::MAINTENANCE)?;
        let r = (|| {
            let more = self.scan(&ctx, sc.end, sc.chain, None)?;
            if let Stop::ReadError(p) = more.stop {
                return Err(ToyError::IoFault(p));
            }
            let mut committed = sc.end;
            for g in more.groups.iter().take_while(|g| !g.durable()) {
                fold_slot(&mut s, g, self.bugs(), false)?;
                committed = g.end;
            }
            s.committed_lsn = committed;
            let mut b = vec![0u8; HEAD_LEN];
            b[..SLOT_LEN].copy_from_slice(&s.to_bytes());
            s.slot_seq = 2;
            b[SLOT_LEN..].copy_from_slice(&s.to_bytes());
            let w = self.vfs.write_at(&self.head, 0, &b);
            self.io(w)
        })();
        if held {
            self.drop_writer();
        }
        r?;
        self.sync_file(&self.head, SyncKind::DataAndMeta);
        self.view = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::testing::{EPOCH, STORE, open, sim_store};
    use crate::{Bug, Bugs, Config};

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
        let h = t.lone_head(1, None).map(|h| (h.n, h.epoch));
        assert_eq!(h, Some((1, EPOCH)));
        assert!(t.lone_head(1, Some(EPOCH)).is_some());
        assert!(t.lone_head(1, Some(EPOCH ^ 1)).is_none(), "another epoch");
        assert!(t.lone_head(2, None).is_none(), "another position");
        assert!(t.lone_head(3, None).is_none(), "no log.3");
        let mut p54 = open(
            &v,
            &cfg.clone()
                .with_bugs(Bugs::only(Bug::P54T13NoPositionCheck)),
        );
        assert_eq!(p54.lone_head(2, None).map(|h| h.n), Some(2));
        assert!(p54.lone_head(1, Some(EPOCH ^ 1)).is_none());
        let mut p55 = open(&v, &cfg.with_bugs(Bugs::only(Bug::P55NoEpochCheck)));
        let h = p55.lone_head(1, Some(EPOCH ^ 1)).map(|h| h.epoch);
        assert_eq!(h, Some(EPOCH), "the head keeps the epoch it carries");
        assert!(p55.lone_head(2, None).is_none());
    }
}
