//! The namespace points of `file mv` and `file rm` over `Vfs` renames and unlinks ([F16] P-16–P-19, P-71, P-82, P-83;
//! [40 §3.4], [40 §3.5]): the plan, the intent slot and anchor ([F03 §8], §10), the `FsIntent` group, the rename or
//! unlink with its directory barriers, the commit with `FsIntentDone`, and intent recovery with the re-barrier.
//!
//! The toy's project files live in directories the harness names ([`Toy::set_project_dirs`]); item `i` of an intent is
//! the file `f<key>` in project directory `src_dir`. A `file rm --trash` item moves to `trash/<intent>/<i>` in the store.

use std::path::{Path, PathBuf};

use moirai_vfs::{
    Access, BootIdentity, LockByte, OpenHint, ProbeResult, RelPath, RootAccess, RootRole,
    ShareRetry, SlotIndex, Vfs, VfsErrorKind,
};

use crate::bugs::Bug;
use crate::codec::{Writer, hash64};
use crate::format::{INTENT_MV, INTENT_RM, INTENT_RM_TRASH, IntentAbortRec, IntentItem, IntentRec};
use crate::init::nonzero_u64;
use crate::ops::{CommitOp, Op};
use crate::state::{IntentState, MAIN};
use crate::store::{Toy, ToyError, rel};
use crate::tap::{Note, Tap};

/// The operation id of the commit that closes an intent ([F05 §9.16]): the intent's key with this bit.
pub const DONE_TAG: u64 = 1 << 63;

/// A `file mv` or `file rm` of one project file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileOp {
    /// The harness's key of the operation (the intent's `key`).
    pub key: u64,
    /// The file (`f<file>` in its directory).
    pub file: u64,
    /// The source directory's index.
    pub src: u8,
    /// `mv`: the destination directory's index; `rm`: 0.
    pub dst: u8,
    /// 1 `mv`, 2 `rm`, 3 `rm --trash`.
    pub op: u8,
}

/// The name of project file `file`.
pub fn file_name(file: u64) -> String {
    format!("f{file}")
}

/// The content digest of a project file (its toy `oid`).
pub fn oid(bytes: &[u8]) -> u64 {
    hash64(bytes)
}

/// The liveness of an intent anchor ([OS/proc §6.2], kind 2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Live {
    /// Its holder is alive.
    Alive,
    /// Dead.
    Dead,
    /// Not decidable: never recovered.
    Unknown,
}

impl<V: Vfs, T: Tap> Toy<V, T> {
    /// Names the project directories: index 1 is `dirs[0]`, and so on.
    pub fn set_project_dirs(&mut self, dirs: &[PathBuf]) {
        self.project = dirs.to_vec();
    }

    fn project_dir(&self, idx: u8) -> Option<&Path> {
        self.project
            .get(usize::from(idx).checked_sub(1)?)
            .map(PathBuf::as_path)
    }

    fn project_root(&self, idx: u8) -> Result<V::Root, ToyError> {
        let d = self
            .project_dir(idx)
            .ok_or(ToyError::Refused("no such project directory"))?;
        self.vfs
            .open_root(d, RootRole::Other, RootAccess::ReadWrite)
            .map_err(ToyError::Io)
    }

    fn read_project(&self, root: &V::Root, name: &str) -> Option<Vec<u8>> {
        let f = self
            .vfs
            .open(
                root,
                RelPath::new(name).ok()?,
                Access::Read,
                OpenHint::Normal,
            )
            .ok()?;
        let size = self.vfs.file_size(&f).ok()?;
        let mut b = vec![0u8; size as usize];
        self.vfs.read_exact_at(&f, 0, &mut b).ok()?;
        Some(b)
    }

    fn exists(&self, root: &V::Root, name: &str) -> bool {
        RelPath::new(name).is_ok_and(|r| self.vfs.path_identity(root, r).is_ok())
    }

    /// `durable-name` on a project directory, with the error policy of [F16] P-91.
    fn sync_root(&self, root: &V::Root) {
        self.durable(self.vfs.sync_dir(root, None), || {
            self.vfs.sync_dir(root, None)
        });
    }

    // ---- the intent slot ([F03 §8], §10) ----

    fn slot_record(&self, i: u8, nonce: u64) -> Vec<u8> {
        let id = self.vfs.self_id();
        let mut w = Writer::with_capacity(128);
        w.u8(1)
            .u8(2)
            .u8(self.vfs.os_tag() as u8)
            .u8(0)
            .u16(u16::from(i))
            .u16(0)
            .u64(nonce)
            .zeros(32)
            .bytes(&id.to_bytes())
            .zeros(16)
            .u64(0)
            .zeros(16);
        let sum = hash64(&w.buf);
        w.u64(sum);
        w.buf
    }

    /// Takes an intent slot: a nonce, the first free slot byte from `nonce mod 256`, its `SlotRec` (SR-2), and the
    /// anchor (kind 2).
    fn take_slot(&mut self) -> Result<(u8, [u8; 32]), ToyError> {
        let nonce = nonzero_u64(&self.vfs);
        for k in 0..256u64 {
            let i = ((nonce + k) % 256) as u16;
            let Some(slot) = SlotIndex::new(i) else {
                continue;
            };
            if self
                .locks
                .try_take(&self.vfs, LockByte::Slot(slot))
                .map_err(|e| ToyError::Lock(e.to_string()))?
            {
                let rec = self.slot_record(i as u8, nonce);
                let data = self.locks.data(&self.vfs);
                if self.vfs.write_at(data, slot.record_offset(), &rec).is_err() {
                    // SR-4: the record cannot be written: release at once, no slot.
                    self.locks.release(&self.vfs, LockByte::Slot(slot));
                    return Err(ToyError::Busy);
                }
                let boot_hash = match self.boot {
                    BootIdentity::Known(b) => b.hash(),
                    BootIdentity::Unknown(_) => 0,
                };
                let mut a = [0u8; 32];
                a[0] = 2;
                a[1] = self.vfs.os_tag() as u8;
                a[2..4].copy_from_slice(&i.to_le_bytes());
                a[8..16].copy_from_slice(&nonce.to_le_bytes());
                a[24..32].copy_from_slice(&boot_hash.to_le_bytes());
                return Ok((i as u8, a));
            }
        }
        Err(ToyError::Busy)
    }

    /// SR-5: zero the record, then release the byte.
    fn free_slot(&mut self, i: u8) {
        if let Some(slot) = SlotIndex::new(u16::from(i)) {
            let data = self.locks.data(&self.vfs);
            let _ = self.vfs.write_at(data, slot.record_offset(), &[0u8; 128]);
            self.locks.release(&self.vfs, LockByte::Slot(slot));
        }
    }

    /// The liveness of intent anchor `a` ([OS/proc §6.2] steps A–D, kind 2). P-71's seeded bug reads `Unknown` as
    /// Dead.
    pub fn anchor_liveness(&self, a: &[u8; 32]) -> Live {
        let live = self.anchor_liveness_exact(a);
        if live == Live::Unknown && self.bugs().on(Bug::P71UnknownAnchorAsDead) {
            return Live::Dead;
        }
        live
    }

    fn anchor_liveness_exact(&self, a: &[u8; 32]) -> Live {
        if a[0] != 2 {
            return Live::Unknown;
        }
        let slot = u16::from_le_bytes([a[2], a[3]]);
        let nonce = u64::from_le_bytes(a[8..16].try_into().unwrap_or([0; 8]));
        let boot_hash = u64::from_le_bytes(a[24..32].try_into().unwrap_or([0; 8]));
        // Step B: a boot change makes every holder Dead.
        if let BootIdentity::Known(b) = self.boot
            && boot_hash != 0
            && boot_hash != b.hash()
        {
            return Live::Dead;
        }
        let Some(i) = SlotIndex::new(slot) else {
            return Live::Dead;
        };
        let data = self.locks.data(&self.vfs);
        let read = |vfs: &V| {
            let mut r = [0u8; 128];
            vfs.read_exact_at(data, i.record_offset(), &mut r)
                .ok()
                .map(|()| r)
        };
        // Step C: LOCK readable.
        let Some(r) = read(&self.vfs) else {
            return Live::Unknown;
        };
        let matches = |r: &[u8; 128]| {
            hash64(&r[..120]) == u64::from_le_bytes(r[120..128].try_into().unwrap_or([0; 8]))
                && r[1] == 2
                && u64::from_le_bytes(r[8..16].try_into().unwrap_or([0; 8])) == nonce
        };
        if !matches(&r) {
            return Live::Dead;
        }
        match self.locks.probe(&self.vfs, LockByte::Slot(i)) {
            ProbeResult::Held => match read(&self.vfs) {
                Some(r2) if matches(&r2) => Live::Alive,
                Some(_) => Live::Dead,
                None => Live::Unknown,
            },
            ProbeResult::Free => Live::Dead,
            ProbeResult::Unknown => Live::Unknown,
        }
    }

    // ---- file mv, file rm ----

    /// `file mv` or `file rm` of one file ([40 §3.4], [40 §3.5], [F16] P-16–P-18, P-82, P-83): plan, slot, intent,
    /// namespace change with its directory barriers, commit with `FsIntentDone`. Returns the intent id.
    pub fn file_op(&mut self, f: &FileOp) -> Result<u64, ToyError> {
        let bugs = self.bugs();
        let name = file_name(f.file);
        let src = self.project_root(f.src)?;
        // Plan: the source exists, the destination does not, both parents on one volume.
        let content = self
            .read_project(&src, &name)
            .ok_or(ToyError::Refused("the source is missing"))?;
        let mut cross = false;
        let dst = if f.op == INTENT_MV {
            let d = self.project_root(f.dst)?;
            if self.exists(&d, &name) {
                return Err(ToyError::Refused("the destination exists"));
            }
            let v1 = self.vfs.root_identity(&src).map_err(ToyError::Io)?.volume;
            let v2 = self.vfs.root_identity(&d).map_err(ToyError::Io)?.volume;
            if v1 != v2 {
                // P-83: a cross-volume move is refused before the intent; its seeded bug copies, then deletes.
                if !bugs.on(Bug::P83CrossVolumeCopy) {
                    return Err(ToyError::Refused("cross_volume"));
                }
                cross = true;
            }
            Some(d)
        } else {
            None
        };
        let (slot, anchor) = self.take_slot()?;
        let plan = Plan {
            src,
            dst,
            name,
            content,
            cross,
        };
        let r = self.file_op_slot(f, &plan, anchor);
        self.free_slot(slot);
        r
    }

    fn file_op_slot(
        &mut self,
        f: &FileOp,
        plan: &Plan<V::Root>,
        anchor: [u8; 32],
    ) -> Result<u64, ToyError> {
        let (src, dst, name, content, cross) = (
            &plan.src,
            plan.dst.as_ref(),
            plan.name.as_str(),
            plan.content.as_slice(),
            plan.cross,
        );
        let bugs = self.bugs();
        let intent = IntentRec {
            op: f.op,
            branch: 0,
            anchor,
            proc: self.vfs.self_id().to_bytes(),
            hlc: 0,
            key: f.key,
            items: vec![IntentItem {
                file: f.file,
                src_dir: f.src,
                dst_dir: if f.op == INTENT_MV { f.dst } else { 0 },
                oid: oid(content),
            }],
        };
        // P-16: the intent is acknowledged (identity check) before the rename; its seeded bug renames after the append.
        let lsn = if bugs.on(Bug::P16RenameBeforeIntentDurable) {
            self.pending_ns = Some(PendingNs {
                op: f.op,
                src: self.project_dir(f.src).map(Path::to_path_buf),
                dst: self.project_dir(f.dst).map(Path::to_path_buf),
                name: name.to_owned(),
            });
            let d = self.run(&Op::Intent(intent));
            self.pending_ns = None;
            d?.result
        } else {
            self.run(&Op::Intent(intent))?.result
        };
        self.note(Note::IntentAcked { key: f.key, lsn });
        let trash = rel(&format!("trash/{lsn}"));
        let changed = match f.op {
            INTENT_MV => {
                let Some(dst) = dst else {
                    return Err(ToyError::Refused("no destination"));
                };
                let rn = RelPath::new(name).map_err(|_| ToyError::Refused("bad name"))?;
                let r = if bugs.on(Bug::P16RenameBeforeIntentDurable) {
                    Ok(())
                } else if cross {
                    // P-83's seeded bug: a copy (written, never flushed), then the source's unlink.
                    self.copy_then_unlink(src, dst, rn, content)
                } else {
                    self.vfs
                        .rename_noreplace(src, rn, dst, rn, ShareRetry::None)
                };
                match r {
                    Ok(()) => {}
                    Err(e) if e.kind == VfsErrorKind::CrossDevice => {
                        self.abort_intent(lsn, 2)?;
                        return Err(ToyError::Refused("cross_volume"));
                    }
                    Err(e)
                        if matches!(
                            e.kind,
                            VfsErrorKind::DiskFull | VfsErrorKind::InsufficientSpace
                        ) =>
                    {
                        // P-90: `DiskFull` aborts the command without an acknowledgement (its seeded bug goes on as if
                        // the rename had succeeded); the intent stays open for intent recovery (P-71).
                        self.io(Err::<(), _>(e))?;
                    }
                    Err(e) => {
                        // The rename failed and the source is still in place: the intent is closed at once
                        // (`FsIntentAborted` reason 1, not renamed, [F05 §9.17]), then the error is the command's.
                        self.abort_intent(lsn, 1)?;
                        return Err(ToyError::Io(e));
                    }
                }
                // P-17: durable-name on both parents; its seeded bug syncs the destination only.
                if !bugs.on(Bug::P17T12MoveWithoutBothParents) {
                    self.sync_root(src);
                }
                self.sync_root(dst);
                true
            }
            INTENT_RM => {
                let rn = RelPath::new(name).map_err(|_| ToyError::Refused("bad name"))?;
                let r = self.vfs.unlink(src, rn, ShareRetry::None);
                self.io(r)?;
                // P-18: durable-name on the parent before the commit; its seeded bug commits first.
                if !bugs.on(Bug::P18RemoveBeforeParentSync) {
                    self.sync_root(src);
                }
                true
            }
            INTENT_RM_TRASH => {
                let _ = self.vfs.create_dir(&self.root, RelPath::literal("trash"));
                self.sync_store_dir(None);
                let r = self.vfs.create_dir(&self.root, trash.as_rel_path());
                self.io(r)?;
                self.sync_store_dir(Some(RelPath::literal("trash")));
                let rn = RelPath::new(name).map_err(|_| ToyError::Refused("bad name"))?;
                let to = rel(&format!("trash/{lsn}/0"));
                let r = self.vfs.rename_noreplace(
                    src,
                    rn,
                    &self.root,
                    to.as_rel_path(),
                    ShareRetry::None,
                );
                self.io(r)?;
                // P-82: every rename is followed by durable-name on every parent; its seeded bug relies on write-through.
                if !bugs.on(Bug::P82WriteThroughWithoutDirFlush) {
                    self.sync_root(src);
                    self.sync_store_dir(Some(trash.as_rel_path()));
                }
                true
            }
            _ => false,
        };
        if !changed {
            return Err(ToyError::Refused("unknown intent op"));
        }
        self.commit_done(lsn, f.key, false)?;
        if f.op == INTENT_RM && bugs.on(Bug::P18RemoveBeforeParentSync) {
            self.sync_root(src);
        }
        Ok(lsn)
    }

    /// The rename (or unlink) of P-16's seeded bug, issued between the intent's append and its phase 2b.
    pub(crate) fn early_namespace(&mut self) {
        let Some(p) = self.pending_ns.take() else {
            return;
        };
        let (Some(src), Some(dst)) = (p.src, p.dst) else {
            return;
        };
        let (Ok(s), Ok(d)) = (
            self.vfs
                .open_root(&src, RootRole::Other, RootAccess::ReadWrite),
            self.vfs
                .open_root(&dst, RootRole::Other, RootAccess::ReadWrite),
        ) else {
            return;
        };
        if p.op == INTENT_MV
            && let Ok(rn) = RelPath::new(&p.name)
            && self
                .vfs
                .rename_noreplace(&s, rn, &d, rn, ShareRetry::None)
                .is_ok()
        {
            self.sync_root(&s);
            self.sync_root(&d);
        }
    }

    fn copy_then_unlink(
        &self,
        src: &V::Root,
        dst: &V::Root,
        rn: RelPath<'_>,
        content: &[u8],
    ) -> Result<(), moirai_vfs::VfsError> {
        let c = self.vfs.create_new(dst, rn)?;
        self.vfs.write_at(&c, 0, content)?;
        drop(c);
        self.vfs.unlink(src, rn, ShareRetry::None)
    }

    /// The commit that closes an intent, with its `FsIntentDone` in the same group ([F05 §4.7]).
    fn commit_done(&mut self, intent_lsn: u64, key: u64, recovered: bool) -> Result<(), ToyError> {
        let op = CommitOp {
            op: key | DONE_TAG,
            digest: key | DONE_TAG,
            ref_name: MAIN,
            key: Some(key | DONE_TAG),
            intent_done: (intent_lsn != 0).then(|| (intent_lsn, recovered, vec![1])),
            ..CommitOp::default()
        };
        self.run(&Op::Commit(op)).map(|_| ())
    }

    fn abort_intent(&mut self, intent_lsn: u64, reason: u8) -> Result<(), ToyError> {
        self.run(&Op::Abort(IntentAbortRec {
            intent_lsn,
            reason,
            aflags: 0,
            hlc: 0,
        }))
        .map(|_| ())
    }

    /// Intent recovery ([F16] P-71, P-19; [40 §3.4] step 5): under the maintenance byte taken by try, every open intent
    /// whose anchor is Dead is decided by the recovery table; a roll-forward first re-establishes the namespace barrier on
    /// every parent (its seeded bug, P-19, skips it). An intent whose anchor is Alive or Unknown is left alone.
    pub fn recover_intents(&mut self) -> Result<(), ToyError> {
        if !self
            .locks
            .try_take(&self.vfs, LockByte::Maintenance)
            .map_err(|e| ToyError::Lock(e.to_string()))?
        {
            return Ok(());
        }
        let r = self.recover_intents_held();
        self.locks.release(&self.vfs, LockByte::Maintenance);
        r
    }

    fn recover_intents_held(&mut self) -> Result<(), ToyError> {
        let bugs = self.bugs();
        let state = self.scratch()?;
        let open: Vec<(u64, IntentRec)> = state
            .intents
            .iter()
            .filter(|(_, r)| r.state == IntentState::Open)
            .map(|(&l, r)| (l, r.rec.clone()))
            .collect();
        for (lsn, rec) in open {
            if self.anchor_liveness(&rec.anchor) != Live::Dead {
                continue;
            }
            let Some(item) = rec.items.first() else {
                continue;
            };
            let name = file_name(item.file);
            let src = self.project_root(item.src_dir)?;
            let src_there = self.exists(&src, &name);
            let (dst_there, dst_root) = match rec.op {
                INTENT_MV => {
                    let d = self.project_root(item.dst_dir)?;
                    (self.exists(&d, &name), Some(d))
                }
                INTENT_RM_TRASH => {
                    let t = format!("trash/{lsn}/0");
                    (self.exists(&self.root, &t), None)
                }
                _ => (false, None),
            };
            match recovery_decision(rec.op, src_there, dst_there) {
                Some(reason) => self.abort_intent(lsn, reason)?,
                None => {
                    // The re-barrier on every parent of the rolled-forward item (P-19).
                    if !bugs.on(Bug::P19T14RollForwardWithoutBarrier) {
                        self.sync_root(&src);
                        if let Some(d) = &dst_root {
                            self.sync_root(d);
                        }
                        if rec.op == INTENT_RM_TRASH {
                            self.sync_store_dir(Some(rel(&format!("trash/{lsn}")).as_rel_path()));
                        }
                    }
                    self.commit_done(lsn, rec.key, true)?;
                }
            }
        }
        Ok(())
    }
}

/// The recovery table of [40 §3.4] step 5 for a one-item intent of `op` whose anchor is Dead: `None` rolls it forward
/// (the namespace change took effect: the commit with `FsIntentDone` is appended, `recovered` = 1), `Some(reason)`
/// aborts it with that `FsIntentAborted` reason. `src_there`: the source name exists; `dst_there`: the destination
/// (`mv`) or the trash entry (`rm --trash`) exists.
///
/// | op | source | destination | outcome |
/// |---|---|---|---|
/// | `rm` | present | — | abort 1 (never removed) |
/// | `rm` | absent | — | roll forward |
/// | `mv`, `rm --trash` | present | absent | abort 1 (never moved) |
/// | `mv`, `rm --trash` | absent | present | roll forward |
/// | `mv`, `rm --trash` | present | present | abort 4 (both names exist: an external actor) |
/// | `mv`, `rm --trash` | absent | absent | abort 5 (neither exists: an external actor) |
pub fn recovery_decision(op: u8, src_there: bool, dst_there: bool) -> Option<u8> {
    match (op, src_there, dst_there) {
        (INTENT_RM, true, _) => Some(1),
        (INTENT_RM, false, _) => None,
        (_, true, false) => Some(1),
        (_, false, true) => None,
        (_, true, true) => Some(4),
        (_, false, false) => Some(5),
    }
}

/// What a `file mv` or `file rm` planned before its intent ([40 §3.4] step 1).
struct Plan<R> {
    /// The source directory.
    src: R,
    /// The destination directory (`mv`).
    dst: Option<R>,
    /// The file's name.
    name: String,
    /// Its content at planning.
    content: Vec<u8>,
    /// P-83's seeded bug: the move crosses volumes and copies.
    cross: bool,
}

/// The namespace change P-16's seeded bug issues early.
#[derive(Clone, Debug)]
pub(crate) struct PendingNs {
    pub(crate) op: u8,
    pub(crate) src: Option<PathBuf>,
    pub(crate) dst: Option<PathBuf>,
    pub(crate) name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recovery_table() {
        // (op, source there, destination there) → outcome.
        let table: [(u8, bool, bool, Option<u8>); 12] = [
            (INTENT_RM, true, false, Some(1)),
            (INTENT_RM, true, true, Some(1)),
            (INTENT_RM, false, false, None),
            (INTENT_RM, false, true, None),
            (INTENT_MV, true, false, Some(1)),
            (INTENT_MV, false, true, None),
            (INTENT_MV, true, true, Some(4)),
            (INTENT_MV, false, false, Some(5)),
            (INTENT_RM_TRASH, true, false, Some(1)),
            (INTENT_RM_TRASH, false, true, None),
            (INTENT_RM_TRASH, true, true, Some(4)),
            (INTENT_RM_TRASH, false, false, Some(5)),
        ];
        for (op, src, dst, want) in table {
            assert_eq!(recovery_decision(op, src, dst), want, "{op} {src} {dst}");
        }
        // Every abort reason is one FsIntentAborted accepts ([F05 §9.17]: 1–5).
        for (op, src, dst, _) in table {
            if let Some(r) = recovery_decision(op, src, dst) {
                assert!((1..=5).contains(&r));
            }
        }
    }

    /// [40 §3.4], [F05 §9.17]: a rename that fails with an error other than `DiskFull` changes nothing ([F15] NS-4),
    /// and the intent is closed at once with `FsIntentAborted` reason 1 (not renamed); the file stays where it was.
    #[test]
    fn a_failed_rename_aborts_its_intent_at_once() {
        use crate::config::Config;
        use crate::testing::{open, sim_store};
        use moirai_vfs_sim::Site;
        let c = Config::test_profile();
        let (w, v, _) = sim_store(&c, 61);
        let dirs = [PathBuf::from("/sim/proj/a"), PathBuf::from("/sim/proj/b")];
        for d in &dirs {
            w.mkdir_all(d);
        }
        w.put_file(&dirs[0].join(file_name(7)), b"seven")
            .unwrap_or_else(|e| panic!("{e:?}"));
        let mut t = open(&v, &c);
        t.set_project_dirs(&dirs);
        // The next namespace operation, the rename, fails with `AccessDenied`.
        w.queue_choice(Site::NsFault, 2);
        let r = t.file_op(&FileOp {
            key: 1,
            file: 7,
            src: 1,
            dst: 2,
            op: INTENT_MV,
        });
        assert!(
            matches!(&r, Err(ToyError::Io(e)) if e.kind == VfsErrorKind::AccessDenied),
            "{r:?}"
        );
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        let states: Vec<IntentState> = st.intents.values().map(|i| i.state).collect();
        assert_eq!(states, vec![IntentState::Aborted { reason: 1 }]);
        assert!(w.exists(&dirs[0].join(file_name(7))));
        assert!(!w.exists(&dirs[1].join(file_name(7))));
    }

    #[test]
    fn file_names_and_digests() {
        assert_eq!(file_name(7), "f7");
        assert_eq!(oid(b"x"), hash64(b"x"));
        assert_eq!(DONE_TAG & 7, 0);
    }
}
