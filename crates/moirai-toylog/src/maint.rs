//! Maintenance ([F16 §12]): the delta checkpoint (the toy writes a full base segment each time) with its retirements,
//! the two-slot barrier after the maintenance's own `Checkpoint` (P-13, P-62), deletion under P-77's conditions, the
//! spare extent (P-96), the quiet bytes (L-8), and `repair --rebuild-from-log` for a damaged segment.

use moirai_vfs::{
    Access, LockByte, N_QUIET, OpenHint, ProbeResult, QuietIndex, RelPath, ShareRetry, SyncKind,
    Vfs, VfsErrorKind,
};

use crate::bugs::Bug;
use crate::format::{
    CK_RELEASED, CK_RETIREMENTS, CK_SET_CHANGE, CheckpointRec, Retirement, SegRef, family,
};
use crate::head::{FLAG_QUIET, Slot};
use crate::init::nonzero_u64;
use crate::ops::Op;
use crate::state::State;
use crate::store::{ScanCtx, Toy, ToyError, log_name, rel, sealed_body, sealed_bytes};
use crate::tap::{Note, StoreFile, Tap};
use crate::write::activity::MAINTENANCE;

/// The sealed files of a store directory ([`Toy::fsck`]).
pub(crate) struct SealedFiles {
    /// The names of the files whose bytes do not match their headers.
    pub(crate) damaged: Vec<String>,
    /// Every sound `hist` file's extent and that extent's bytes.
    pub(crate) hists: Vec<(u32, Vec<u8>)>,
}

/// What one checkpoint did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Checkpointed {
    /// The new `checkpoint_lsn`.
    pub upto: u64,
    /// The new base segment's number.
    pub segment: u32,
    /// The extents it retired.
    pub retired: Vec<u32>,
    /// The files it deleted.
    pub deleted: Vec<String>,
}

impl<V: Vfs, T: Tap> Toy<V, T> {
    /// Whether quiet mode is in effect ([F03 §3.1] rule 2): `HEAD.flags` bit 0, or a probe of any quiet byte answering
    /// `Held` or `Unknown`. L-8's seeded bug probes only quiet 0.
    pub fn quiet(&self, s: &Slot) -> bool {
        if s.flags & FLAG_QUIET != 0 {
            return true;
        }
        let n = if self.bugs().on(Bug::L08ProbeFirstQuietOnly) {
            1
        } else {
            N_QUIET
        };
        (0..n).any(|k| {
            QuietIndex::new(k).is_some_and(|q| {
                !matches!(
                    self.locks.probe(&self.vfs, LockByte::Quiet(q)),
                    ProbeResult::Free
                )
            })
        })
    }

    /// Takes a quiet byte of this process's own for its run ([F03 §3.1] rule 4): quiet 0, then 1 … 8, by `try_acquire`.
    pub fn hold_quiet(&mut self) -> Result<Option<u8>, ToyError> {
        for k in 0..N_QUIET {
            let Some(q) = QuietIndex::new(k) else {
                continue;
            };
            if self
                .locks
                .try_take(&self.vfs, LockByte::Quiet(q))
                .map_err(|e| ToyError::Lock(e.to_string()))?
            {
                return Ok(Some(k));
            }
        }
        Ok(None)
    }

    /// Releases quiet byte `k`.
    pub fn release_quiet(&mut self, k: u8) {
        if let Some(q) = QuietIndex::new(k) {
            self.locks.release(&self.vfs, LockByte::Quiet(q));
        }
    }

    /// One automatic maintenance run ([F16] P-76): the maintenance byte by `try_acquire` (Busy skips), quiet mode
    /// defers it (L-8), then a checkpoint, the barrier, the deletions and the spare extent. P-76's seeded bug runs
    /// without the maintenance byte. Returns `None` when it skipped.
    pub fn checkpoint(&mut self) -> Result<Option<Checkpointed>, ToyError> {
        let bugs = self.bugs();
        let got = if bugs.on(Bug::P76MaintenanceWithoutByte) {
            false
        } else {
            self.locks
                .try_take(&self.vfs, LockByte::Maintenance)
                .map_err(|e| ToyError::Lock(e.to_string()))?
        };
        if !got && !bugs.on(Bug::P76MaintenanceWithoutByte) {
            return Ok(None);
        }
        let r = self.checkpoint_held();
        if got {
            self.locks.release(&self.vfs, LockByte::Maintenance);
        }
        r
    }

    fn checkpoint_held(&mut self) -> Result<Option<Checkpointed>, ToyError> {
        let bugs = self.bugs();
        let s = self.refresh()?;
        // [F03 §3.1] rule 2: quiet mode is evaluated once per maintenance decision, by one probe round of the quiet
        // bytes, before the run's first record. Every maintenance run of the toy is an automatic delta checkpoint below
        // the cap of [F17 §5.3], the kind quiet mode defers; a decided run may complete although a requester takes a
        // quiet byte meanwhile.
        if self.quiet(&s) {
            return Ok(None);
        }
        self.note(Note::MaintenanceDecided {
            automatic: true,
            below_cap: true,
        });
        let ctx = ScanCtx::of(&s);
        // P-80: fold up to a group boundary at or below the published committed_lsn. The toy folds up to the published
        // durable_lsn, so that a segment never holds a lazy group that a crash may still take back from the log.
        let (state, u) = self.state_upto(&ctx, &s, s.durable_lsn)?;
        if u <= s.checkpoint_lsn {
            self.prepare_spare(&s)?;
            return Ok(None);
        }
        // P-78: the file number, with create-new semantics.
        let mut g = s.counters.next_file_no.max(state.counters.next_file_no);
        for &(_, no) in &state.files {
            g = g.max(no + 1);
        }
        // The digest covers the body only, so it does not change with the number write_sealed settles on.
        let (bytes, digest) = sealed_bytes(family::SEG_BASE, g, &state.snapshot(u));
        let g = self.write_sealed(family::SEG_BASE, g, &bytes)?;
        let mut next_no = g + 1;
        // P-73: retire the oldest extents while more than store.log-active-extents are unretired — the extents up to the
        // one that holds the end of the published log now, at the decision point ([F05 §2.5] EX-1, [F17 §4.2]) — each
        // only when every record in it lies below the new checkpoint_lsn and checkpoint_lsn > n·E (EX-4, EX-5). The
        // seeded bug checks EX-5 against the extent's first byte, (n − 1)·E, and so retires the extent that holds
        // checkpoint_lsn.
        let mut active = s.active_log;
        let (now, _) = self.read_head()?;
        let last = ctx.extent(now.committed_lsn.max(u).saturating_sub(1).max(s.epoch_lsn));
        let mut retirements = Vec::new();
        while last + 1 - active > self.cfg.active_extents {
            let end_n = ctx.start_of(active) + ctx.e;
            let ok = if bugs.on(Bug::P73RetireAtBoundary) {
                u > ctx.start_of(active)
            } else {
                u > end_n
            };
            if !ok {
                break;
            }
            let Some(body) = self.extent_bytes(&ctx, active)? else {
                break;
            };
            let mut hb = active.to_le_bytes().to_vec();
            hb.extend_from_slice(&body);
            let (bytes, d) = sealed_bytes(family::HIST, next_no, &hb);
            let h = self.write_sealed(family::HIST, next_no, &bytes)?;
            next_no = h + 1;
            retirements.push(Retirement {
                extent: active,
                hist_file: h,
                total_len: bytes.len() as u64,
                digest: d,
            });
            active += 1;
        }
        let released: Vec<(u8, u32)> = s
            .segments
            .iter()
            .map(|x| (family::SEG_BASE, x.file_no))
            .collect();
        let mut ckflags = CK_SET_CHANGE;
        if !retirements.is_empty() {
            ckflags |= CK_RETIREMENTS;
        }
        if !released.is_empty() {
            ckflags |= CK_RELEASED;
        }
        let rec = CheckpointRec {
            ckflags,
            append_hlc: 0,
            next_file_no: next_no,
            upto_lsn: u,
            active_log: active,
            segments: vec![SegRef {
                file_no: g,
                kind: 1,
                upto_lsn: u,
                digest,
            }],
            retirements: retirements.clone(),
            released,
        };
        self.run(&Op::Checkpoint(rec))?;
        // P-14's seeded bug deletes before the barrier's HEAD flush.
        let mut deleted = Vec::new();
        if bugs.on(Bug::P14T6DeleteBeforeBarrier) {
            deleted = self.delete_released()?;
        }
        // P-62, P-13: the barrier, after the maintenance's own Checkpoint passed its identity check (run() returned).
        if !bugs.on(Bug::P62G12BarrierBeforeCheckpoint) {
            self.durable_publish(MAINTENANCE, None, crate::write::Change::default())?;
        }
        if !bugs.on(Bug::P14T6DeleteBeforeBarrier) {
            deleted = self.delete_released()?;
        }
        let (s2, _) = self.read_head()?;
        self.prepare_spare(&s2)?;
        Ok(Some(Checkpointed {
            upto: u,
            segment: g,
            retired: retirements.iter().map(|r| r.extent).collect(),
            deleted,
        }))
    }

    /// The state folded up to the group boundary `limit` (at or below the view's bound): the view's own when it ends
    /// there, else the newest set's state and the groups after it up to `limit`.
    fn state_upto(
        &mut self,
        ctx: &ScanCtx,
        s: &Slot,
        limit: u64,
    ) -> Result<(State, u64), ToyError> {
        if let Some(v) = self.view.as_ref()
            && v.l0 == limit
        {
            return Ok((v.state.clone(), v.l0));
        }
        let (mut state, upto) = self.load_set(s)?;
        if upto >= limit {
            return Ok((state, upto));
        }
        let chain = self.chain_at(ctx, upto, s.durable_lsn)?;
        let bugs = self.bugs();
        let end = self.scan_each(ctx, upto, chain, Some(limit), false, &mut |g| {
            state.apply(&g, bugs, false).map_err(ToyError::from)
        })?;
        Ok((state, end.end))
    }

    /// The barrier of P-62's seeded bug: run between the append of the maintenance's `Checkpoint` and its phase 2b.
    pub(crate) fn early_barrier(&mut self) {
        let _ = self.durable_publish(MAINTENANCE, None, crate::write::Change::default());
    }

    /// Writes a sealed file under its final number ([F16] P-10, P-78): create-new (the next number if the name exists),
    /// write, `durable+meta`, seal, `durable-name` on the store directory. P-10's seeded bug skips `durable+meta`;
    /// P-90's treats a `DiskFull` of the create (or of the write) as success, so the record then names a file that was
    /// never written. Returns the number used.
    pub(crate) fn write_sealed(
        &mut self,
        fam: u8,
        mut no: u32,
        bytes: &[u8],
    ) -> Result<u32, ToyError> {
        let f = loop {
            let name = crate::store::sealed_name(fam, no);
            match self.vfs.create_new(&self.root, name.as_rel_path()) {
                Ok(f) => break f,
                Err(e) if e.kind == VfsErrorKind::AlreadyExists => no += 1,
                Err(e) => {
                    self.io::<()>(Err(e))?;
                    return Ok(no);
                }
            }
        };
        // A different number changes the header: re-encode it.
        let fixed;
        let bytes = if sealed_body(bytes, fam, no, None).is_some() {
            bytes
        } else {
            let body = &bytes[crate::store::SEALED_HDR..];
            fixed = sealed_bytes(fam, no, body).0;
            &fixed[..]
        };
        let r = self.vfs.write_at(&f, 0, bytes);
        self.io(r)?;
        if !self.bugs().on(Bug::P10CheckpointBeforeSegmentDurable) {
            self.sync_file(&f, SyncKind::DataAndMeta);
        }
        let r = self.vfs.seal(&f);
        self.io(r)?;
        self.sync_store_dir(None);
        Ok(no)
    }

    /// The bytes of `log.<n>` (a whole extent), for its `hist` copy.
    fn extent_bytes(&mut self, ctx: &ScanCtx, n: u32) -> Result<Option<Vec<u8>>, ToyError> {
        if !self.open_extent(n)? {
            return Ok(None);
        }
        let Some(f) = self.ext(n) else {
            return Ok(None);
        };
        let mut b = vec![0u8; ctx.e as usize];
        match self.vfs.read_exact_at(f, 0, &mut b) {
            Ok(()) => Ok(Some(b)),
            Err(_) => Ok(None),
        }
    }

    /// Deletes every store file that [F16] P-77 allows, after the barrier (condition 5, the caller's order):
    /// 1. a covered `Checkpoint` released it — a base segment in a `released` list, an extent in a retirement entry — as
    ///    the published view (the replay up to `committed_lsn`) shows;
    /// 2. the newest slot does not name it (its segment set; for an extent, a number below its `active_log`), and no
    ///    record of the scanned valid log names it, pending groups included (a `Checkpoint` that named it and was not
    ///    followed by one that released it);
    /// 3. no pin of the scanned valid log references it, pending groups included;
    /// 4. `gc.delete-grace` has elapsed since the `append_hlc` of the newest `Checkpoint`, measured by P-89.
    ///
    /// `unlink` (no share retry), then `durable-name` on the store directory. Seeded bugs: P-77 ignores pins; P-74
    /// zero-fills a retired extent and keeps it under its old number.
    fn delete_released(&mut self) -> Result<Vec<String>, ToyError> {
        let bugs = self.bugs();
        let (s, _) = self.read_head()?;
        let ctx = ScanCtx::of(&s);
        let scanned = self.scratch()?;
        let Some(view) = self.view.as_ref() else {
            return Err(ToyError::Corrupt("no view".to_owned()));
        };
        let pins = |st: &State| -> Vec<(u8, u32)> {
            st.pins
                .values()
                .flat_map(|p| p.files.iter().copied())
                .collect()
        };
        let pinned: Vec<(u8, u32)> = if bugs.on(Bug::P77DeletePinnedFile) {
            Vec::new()
        } else {
            pins(&scanned)
        };
        let released = |fam: u8, n: u32| match fam {
            family::LOG => view.state.retired.contains_key(&n),
            _ => view.state.released.contains(&(fam, n)),
        };
        // The grace ([F16] P-77 condition 4, P-89): elapsed on the HLC since the newest Checkpoint's append_hlc.
        let now = moirai_vfs::hlc_next(self.vfs_wall(), 0)
            .max(s.counters.hlc_seq)
            .max(s.counters.hlc_commit);
        let t0 = scanned.last_checkpoint_hlc;
        if (now >> 16).saturating_sub(t0 >> 16) < self.cfg.delete_grace_ms {
            return Ok(Vec::new());
        }
        let entries = self.vfs.list_dir(&self.root, None).map_err(ToyError::Io)?;
        let mut targets: Vec<(String, u8, u32)> = Vec::new();
        for e in entries {
            let Some(name) = e.name.as_segment().map(str::to_owned) else {
                continue;
            };
            let (fam, n) = if let Some(n) = name
                .strip_prefix("seg.base.")
                .and_then(|x| x.parse::<u32>().ok())
            {
                if s.segments.iter().any(|x| x.file_no == n) {
                    continue;
                }
                (family::SEG_BASE, n)
            } else if let Some(n) = name
                .strip_prefix("log.")
                .and_then(|x| x.parse::<u32>().ok())
            {
                if n >= s.active_log {
                    continue;
                }
                (family::LOG, n)
            } else {
                continue;
            };
            if !released(fam, n) || scanned.files.contains(&(fam, n)) || pinned.contains(&(fam, n))
            {
                continue;
            }
            targets.push((name, fam, n));
        }
        let mut out = Vec::new();
        for (name, fam, n) in targets {
            if fam == family::LOG && bugs.on(Bug::P74T1ReuseRetiredExtent) {
                if matches!(self.open_extent(n), Ok(true))
                    && let Some(f) = self.ext(n)
                {
                    let r = self.vfs.recycle_extent(f, ctx.e, &self.vol);
                    self.io(r)?;
                    out.push(name);
                }
                continue;
            }
            let r = self.vfs.unlink(
                &self.root,
                RelPath::new(&name).map_err(|_| ToyError::Corrupt(name.clone()))?,
                ShareRetry::None,
            );
            match r {
                Ok(()) => {
                    if fam == family::LOG {
                        self.forget_extent(n);
                    }
                    out.push(name);
                }
                // A sharing violation, a delete-pending file or an absent one is harmless; the next run repeats it.
                Err(e)
                    if matches!(
                        e.kind,
                        VfsErrorKind::NotFound
                            | VfsErrorKind::SharingViolation
                            | VfsErrorKind::DeletePending
                            | VfsErrorKind::AccessDenied
                    ) => {}
                Err(e) => {
                    self.io::<()>(Err(e))?;
                }
            }
        }
        if !out.is_empty() {
            self.sync_store_dir(None);
        }
        Ok(out)
    }

    fn vfs_wall(&self) -> i64 {
        moirai_vfs::Clock::wall_ms(&self.vfs)
    }

    /// The spare extent ([F16] P-96): when the end of the valid log lies at or beyond E / 2 of its extent n and no
    /// log.<n+1> exists, prepare it under a temporary name and rename it into place, with `durable+meta` and
    /// `durable-name` on `tmp/` and on the store directory. The end of the valid log is found by a scan from `s`'s
    /// `durable_lsn` (pending groups included); a read error there ends the scan where it stands.
    pub(crate) fn prepare_spare(&mut self, s: &Slot) -> Result<(), ToyError> {
        let ctx = ScanCtx::of(s);
        let chain = self.chain_at(&ctx, s.durable_lsn, s.durable_lsn)?;
        let end = self
            .scan_each(&ctx, s.durable_lsn, chain, None, false, &mut |_| Ok(()))?
            .end;
        let n = ctx.extent(end);
        if ctx.offset(end) < ctx.e / 2 || self.extent(n + 1)?.is_some() {
            return Ok(());
        }
        let tmp = rel(&format!("tmp/extent.{}", nonzero_u64(&self.vfs)));
        let vol = self.vol;
        let r = self
            .vfs
            .create_extent(&self.root, tmp.as_rel_path(), ctx.e, &vol);
        let f = match r {
            Ok(f) => f,
            Err(e) => {
                self.io::<()>(Err(e))?;
                return Ok(());
            }
        };
        self.sync_file(&f, SyncKind::DataAndMeta);
        drop(f);
        let r = self.vfs.rename_noreplace(
            &self.root,
            tmp.as_rel_path(),
            &self.root,
            log_name(n + 1).as_rel_path(),
            ShareRetry::None,
        );
        match r {
            Ok(()) => {}
            Err(e) if e.kind == VfsErrorKind::AlreadyExists => {
                // A rotation made the extent ready first: delete the temporary.
                let _ = self
                    .vfs
                    .unlink(&self.root, tmp.as_rel_path(), ShareRetry::None);
                self.sync_store_dir(Some(RelPath::literal("tmp")));
                return Ok(());
            }
            Err(e) => {
                self.io::<()>(Err(e))?;
                return Ok(());
            }
        }
        self.sync_store_dir(Some(RelPath::literal("tmp")));
        self.sync_store_dir(None);
        Ok(())
    }

    /// `doctor --fsck` over the sealed files, a diagnosis that changes nothing ([80 §2.5] rule 8): the names of every
    /// `seg.base.*` and `hist.*` file whose bytes do not match its header.
    pub fn fsck(&mut self) -> Result<Vec<String>, ToyError> {
        Ok(self.sealed_files()?.damaged)
    }

    /// `doctor --verify`'s state ([F16 §17.2] "model"): the log of the newest slot's epoch replayed from the epoch's first
    /// byte into a fresh state that keeps its raw facts, up to the end of the valid log, pending groups included; the
    /// groups of an extent below the slot's `active_log` (retired) come from its `hist` file. It reads the bytes of the
    /// log and of the `hist` files, never a segment snapshot, so no snapshot defect can hide a record from
    /// [`crate::verify()`]. An error when the log cannot be replayed from the epoch's start (for example a `hist` file an
    /// external actor damaged, [F15] FM-10). Like every first read of a process, it follows the boot check ([F16] P-60).
    pub fn doctor_state(&mut self) -> Result<State, ToyError> {
        let s = self.head_for_read()?;
        let ctx = ScanCtx::of(&s);
        let SealedFiles { hists, .. } = self.sealed_files()?;
        self.hist = hists
            .into_iter()
            .filter(|&(n, _)| n < s.active_log)
            .collect();
        let bugs = self.bugs();
        let r = (|| {
            let mut st = State::new().keeping_facts(true);
            let chain = self.chain_at(&ctx, ctx.epoch_lsn, s.durable_lsn)?;
            self.scan_each(&ctx, ctx.epoch_lsn, chain, None, false, &mut |g| {
                st.apply(&g, bugs, false).map_err(ToyError::from)
            })?;
            Ok(st)
        })();
        self.hist.clear();
        r
    }

    /// The sealed files of the store directory, checked against their headers.
    pub(crate) fn sealed_files(&mut self) -> Result<SealedFiles, ToyError> {
        let entries = self.vfs.list_dir(&self.root, None).map_err(ToyError::Io)?;
        let mut damaged = Vec::new();
        let mut hists: Vec<(u32, Vec<u8>)> = Vec::new();
        for e in entries {
            let Some(name) = e.name.as_segment().map(str::to_owned) else {
                continue;
            };
            let (fam, no) =
                if let Some(n) = name.strip_prefix("seg.base.").and_then(|x| x.parse().ok()) {
                    (family::SEG_BASE, n)
                } else if let Some(n) = name.strip_prefix("hist.").and_then(|x| x.parse().ok()) {
                    (family::HIST, n)
                } else {
                    continue;
                };
            self.note(Note::Uses(StoreFile::Sealed { family: fam, no }));
            let ok = self
                .read_file(rel(&name).as_rel_path())
                .and_then(|b| sealed_body(&b, fam, no, None).map(<[u8]>::to_vec));
            match ok {
                None => damaged.push(name),
                Some(body) if fam == family::HIST && body.len() >= 4 => {
                    let n = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                    hists.push((n, body[4..].to_vec()));
                }
                Some(_) => {}
            }
        }
        Ok(SealedFiles { damaged, hists })
    }

    /// Plain `moirai repair` ([F16] P-85, [80 §2.5] rule 8). A store whose `HEAD` has no valid slot or a fatal slot is
    /// repaired from its extent heads first: neither slot is trusted, and both are rebuilt ([F04 §7], [F15] OP-1; spec
    /// sync 2b S2B-P-28). Then `doctor --fsck` over the sealed files, and `repair --rebuild-from-log` of a damaged base
    /// segment: when the newest slot's base segment does not match its header, it is rebuilt from the retired extents'
    /// `hist` files and the log and published by a `Checkpoint`, followed by the barrier. Returns the damaged file names.
    pub fn repair(&mut self) -> Result<Vec<String>, ToyError> {
        // `HEAD` is read once: a slot sector that a failed flush poisoned reads differently on every read (FM-3.2) until
        // it is written again, and `repair_head` rebuilds both slots whatever a read shows.
        let s = match self.read_head() {
            Err(ToyError::NoValidSlot | ToyError::FatalSlot(_)) => {
                self.repair_head()?;
                self.read_head()?.0
            }
            r => r?.0,
        };
        let SealedFiles { damaged, hists } = self.sealed_files()?;
        let Some(seg) = s.segments.first().copied() else {
            return Ok(damaged);
        };
        if !damaged.contains(&format!("seg.base.{}", seg.file_no)) {
            return Ok(damaged);
        }
        // Rebuild the set's state from the log under the maintenance byte ([F16] P-68, P-85): the retired extents from
        // their hist files, then the active log.
        if !self
            .locks
            .try_take(&self.vfs, LockByte::Maintenance)
            .map_err(|e| ToyError::Lock(e.to_string()))?
        {
            return Err(ToyError::Busy);
        }
        let r = self.rebuild_segment(&s, seg, hists);
        self.locks.release(&self.vfs, LockByte::Maintenance);
        r?;
        Ok(damaged)
    }

    fn rebuild_segment(
        &mut self,
        s: &Slot,
        seg: SegRef,
        hists: Vec<(u32, Vec<u8>)>,
    ) -> Result<(), ToyError> {
        let ctx = ScanCtx::of(s);
        self.hist = hists.into_iter().collect();
        let r = self
            .rebuild(&ctx, s.checkpoint_lsn)
            .and_then(|st| Ok((st, self.chain_at(&ctx, s.checkpoint_lsn, s.durable_lsn)?)));
        self.hist.clear();
        let (state, chain) = r?;
        let g = s.counters.next_file_no.max(state.counters.next_file_no);
        let (bytes, digest) = sealed_bytes(family::SEG_BASE, g, &state.snapshot(s.checkpoint_lsn));
        let g = self.write_sealed(family::SEG_BASE, g, &bytes)?;
        let rec = CheckpointRec {
            ckflags: CK_SET_CHANGE | CK_RELEASED,
            append_hlc: 0,
            next_file_no: g + 1,
            upto_lsn: s.checkpoint_lsn,
            active_log: s.active_log,
            segments: vec![SegRef {
                file_no: g,
                kind: 1,
                upto_lsn: s.checkpoint_lsn,
                digest,
            }],
            retirements: Vec::new(),
            released: vec![(family::SEG_BASE, seg.file_no)],
        };
        // The writer's own view cannot be built from the damaged set: build it from the rebuilt state.
        self.view = Some(crate::store::View {
            state,
            l0: s.checkpoint_lsn,
            chain,
            slot_seq: s.slot_seq,
            base: s.checkpoint_lsn,
        });
        self.run(&Op::Checkpoint(rec))?;
        self.durable_publish(MAINTENANCE, None, crate::write::Change::default())?;
        Ok(())
    }

    /// Replays the whole log of the epoch up to `upto` into a fresh state (the retired extents come from `self.hist`).
    fn rebuild(&mut self, ctx: &ScanCtx, upto: u64) -> Result<State, ToyError> {
        let mut st = self.fresh_state();
        let chain = self.chain_at(ctx, ctx.epoch_lsn, ctx.epoch_lsn)?;
        let bugs = self.bugs();
        let end = self.scan_each(ctx, ctx.epoch_lsn, chain, Some(upto), false, &mut |g| {
            st.apply(&g, bugs, false).map_err(ToyError::from)
        })?;
        if end.end < upto {
            return Err(ToyError::Corrupt(format!(
                "the log ends at {} below the segment bound {upto}; the store cannot be rebuilt",
                end.end
            )));
        }
        Ok(st)
    }

    /// `doctor --verify`'s pin check: the files of every live ref's pin that are missing (P-77, P-79, P-81).
    pub fn missing_pinned_files(&mut self) -> Result<Vec<String>, ToyError> {
        let state = self.scratch()?;
        let mut missing = Vec::new();
        for (r, p) in &state.pins {
            if state.refs.get(r).is_some_and(|x| x.deleted) {
                continue;
            }
            for &(fam, no) in &p.files {
                let name = crate::store::sealed_name(fam, no);
                let present = self
                    .vfs
                    .open(
                        &self.root,
                        name.as_rel_path(),
                        Access::Read,
                        OpenHint::Normal,
                    )
                    .is_ok();
                if !present {
                    missing.push(name.as_str().to_owned());
                }
            }
        }
        Ok(missing)
    }
}

/// Maintenance on the in-memory `Vfs`, one handle at a time.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ops::{CommitOp, ForkOp, RuntimeOp};
    use crate::state::MAIN;
    use crate::store::seg_name;
    use crate::testing::{open, sim_store};
    use moirai_vfs::StoreFs;

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

    fn cfg() -> Config {
        let mut c = Config::test_profile();
        c.delete_grace_ms = 0;
        c.facts = true;
        c
    }

    #[test]
    fn a_checkpoint_seals_its_segment_names_it_in_both_slots_and_deletes_the_released_one() {
        let c = cfg();
        let (_w, v, _) = sim_store(&c, 31);
        let mut t = open(&v, &c);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        let c1 = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the first checkpoint ran"));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.segments.len(), 1);
        assert_eq!(
            (s.segments[0].file_no, s.checkpoint_lsn),
            (c1.segment, c1.upto)
        );
        // The barrier: the other slot names the same set.
        let other = t.other_slot().unwrap_or_else(|| panic!("two valid slots"));
        assert_eq!(other.segments, s.segments);
        // A second checkpoint releases the first segment and deletes it after its barrier.
        t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        let c2 = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the second checkpoint ran"));
        let old = format!("seg.base.{}", c1.segment);
        assert!(c2.deleted.contains(&old), "{:?}", c2.deleted);
        assert!(t.read_file(seg_name(c1.segment).as_rel_path()).is_none());
        // A fresh handle reads the history back from the new set, and the raw facts carried in it verify.
        let mut u = open(&v, &c);
        let st = u.read().unwrap_or_else(|e| panic!("{e}")).clone();
        assert!(st.commits.contains_key(&1) && st.commits.contains_key(&2));
        assert!(st.released.contains(&(family::SEG_BASE, c1.segment)));
        assert_eq!(crate::verify(&st), Vec::<String>::new());
    }

    #[test]
    fn a_pinned_segment_is_not_deleted() {
        let c = cfg();
        let (_w, v, _) = sim_store(&c, 32);
        let mut t = open(&v, &c);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        let c1 = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the checkpoint ran"));
        t.run(&Op::Fork(ForkOp {
            op: 5,
            from: MAIN,
            name: 0xF1,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        let c2 = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the second checkpoint ran"));
        assert!(c2.deleted.is_empty(), "{:?}", c2.deleted);
        assert!(t.read_file(seg_name(c1.segment).as_rel_path()).is_some());
        assert_eq!(
            t.missing_pinned_files().unwrap_or_else(|e| panic!("{e}")),
            Vec::<String>::new()
        );
    }

    /// `doctor --verify`'s state is replayed from the log and the `hist` files ([F16 §17.2] "model"): after a retirement
    /// and the deletion of the retired extent it holds the same facts as the reader's view, which starts from the
    /// segment snapshot, and they verify.
    #[test]
    fn doctor_replays_the_facts_from_the_log_and_the_hist_files() {
        let mut c = cfg();
        c.active_extents = 1;
        let e = c.extent_bytes;
        let (_w, v, _) = sim_store(&c, 35);
        let mut t = open(&v, &c);
        let batch = |op: u64| {
            Op::Runtime(RuntimeOp {
                op,
                rows: vec![(op, op)],
                pad: (e / 2) as u32,
                symbols: Vec::new(),
                target_len: 0,
            })
        };
        for o in [batch(7), commit(1), batch(8), commit(2)] {
            t.run(&o).unwrap_or_else(|e| panic!("{e}"));
        }
        let ck = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the checkpoint ran"));
        assert_eq!(ck.retired, [1]);
        assert!(ck.deleted.contains(&"log.1".to_owned()), "{:?}", ck.deleted);
        t.run(&commit(3)).unwrap_or_else(|e| panic!("{e}"));
        let replayed = t.doctor_state().unwrap_or_else(|e| panic!("{e}"));
        let view = t.scratch().unwrap_or_else(|e| panic!("{e}"));
        assert!(replayed.commits.contains_key(&1) && replayed.commits.contains_key(&3));
        assert_eq!(replayed.facts, view.facts);
        assert_eq!(crate::verify(&replayed), Vec::<String>::new());
    }

    #[test]
    fn quiet_mode_defers_maintenance() {
        let c = cfg();
        let (_w, v, _) = sim_store(&c, 33);
        let mut t = open(&v, &c);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        // A quiet byte held by another handle of the process.
        let mut q = open(&v, &c);
        let k = q
            .hold_quiet()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("a quiet byte"));
        assert_eq!(t.checkpoint().unwrap_or_else(|e| panic!("{e}")), None);
        q.release_quiet(k);
        // HEAD.flags quiet.
        t.set_quiet(true).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(t.checkpoint().unwrap_or_else(|e| panic!("{e}")), None);
        t.set_quiet(false).unwrap_or_else(|e| panic!("{e}"));
        assert!(t.checkpoint().unwrap_or_else(|e| panic!("{e}")).is_some());
    }

    #[test]
    fn a_spare_is_prepared_once_the_log_passes_half_its_extent() {
        let c = cfg();
        let (_w, v, _) = sim_store(&c, 34);
        let mut t = open(&v, &c);
        let e = c.extent_bytes;
        t.run(&Op::Runtime(RuntimeOp {
            op: 7,
            rows: vec![(7, 7)],
            pad: (e / 2) as u32,
            symbols: Vec::new(),
            target_len: 0,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(!t.open_extent(2).unwrap_or_else(|e| panic!("{e}")));
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        t.checkpoint().unwrap_or_else(|e| panic!("{e}"));
        assert!(t.open_extent(2).unwrap_or_else(|e| panic!("{e}")));
        let f = t.ext(2).unwrap_or_else(|| panic!("log.2"));
        assert_eq!(t.vfs.file_size(f).unwrap_or_else(|e| panic!("{e}")), e);
        // The next commit rotates into the spare and reads back.
        t.run(&Op::Runtime(RuntimeOp {
            op: 8,
            rows: vec![(8, 8)],
            pad: (e / 2) as u32,
            symbols: Vec::new(),
            target_len: 0,
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert!(s.committed_lsn > e, "{}", s.committed_lsn);
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&2) && st.runtime.get(&8) == Some(&8));
    }
}
