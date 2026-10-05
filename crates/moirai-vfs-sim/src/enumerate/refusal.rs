//! The avail family ([F16 §17.2]): typed refusals, the facts of a run's injected faults read from its trace, and the
//! allowance that judges a refusal against them.
//!
//! A refusal is a correct answer only after a state that the protocol may refuse given the faults the run injected:
//! the enumerator, not the subject, decides which. [`Facts`] collects those faults from the replayable trace — the
//! adversary's fault choices, failed flushes, death-resolved calls, external acts (FM-10), read errors, and the slot
//! writes a failure, a death or a crash cut ([`crate::NOTE_SLOT_WRITE`]) — and [`Allow::check`] answers, for each
//! [`Refusal`], whether a fault the run injected explains it:
//!
//! | Refusal | A correct answer only when the run had |
//! |---|---|
//! | [`Refusal::NoValidSlot`] | a failed flush of a slot file (OP-1, FM-3.3); or a slot write that failed or was cut (FM-5.2, §2.5) and a crash after it, or one that covered both slots ([F04 §8.1] "Both slots absent", [F16] P-61); or an FM-10 act on a slot file, during the run or declared by the setup |
//! | [`Refusal::FatalSlot`] | an FM-10 act on a slot file, during the run or declared by the setup ([F04 §7]: a fatal slot shows a defective writer otherwise) |
//! | [`Refusal::Damaged`] | an FM-10 act on the file it names, a read error (FM-12) or a sharing violation (FM-8.2) ([F16] P-59, P-68; G-13) |
//! | [`Refusal::Corrupt`] | an FM-10 act on a file it concerns ([`Refused::concerning`]), a read error or a sharing violation: a correct flush holder re-writes before it flushes (FM-3.5), so neither its own scan nor any later one reads a shorter log than a successful flush covered, and [F16] P-48's refusal needs one of these too |
//! | [`Refusal::IoFault`] | a read error (FM-12, [F16] P-92) |
//! | [`Refusal::DiskFull`], [`Refusal::Io`] | a fault of a write, flush, `sync_dir`, create or namespace operation (FM-5, FM-3) |
//! | [`Refusal::Lock`] | a replaced `LOCK` (FM-10.3), or another FM-10 act on a file it concerns |
//! | [`Refusal::NotAStore`] | no operation the store must keep (nothing acknowledged survives in it) |
//! | [`Refusal::OutcomeUnknown`] | a failed flush or a failed read ([F16] P-47; [F16 §17.2] avail) |
//! | [`Refusal::Busy`] | for a workload operation, always (live holders, pauses: FM-6); for the recovery, a byte a dead process holds beyond every wait bound or never releases (FM-8.1 classes (b), (c)) |
//!
//! **An FM-10 act explains a refusal only through the file it touched** ([F15] FM-10 "Crash gates": under an external
//! rewrite the gate asserts detection, which is a refusal or a diagnosis that names the file). The files a refusal
//! concerns are the one [`Refusal::Damaged`] names and those of [`Refused::concerning`]: the files the refusing state
//! names or references where it refused (the extent that holds the invalid group, the slot file, the damaged file).
//! - An act during the run (the enumerator's own injection, [`Facts`]) explains a refusal that concerns its file, by
//!   the path that named the file then or its node. A refusal that names no file may concern any: any act during the
//!   run explains it.
//! - A setup fault ([`super::Ledger::setup_fault`]: an FM-10 act before the run, which the subject declares) explains
//!   only a refusal that concerns its file, and, on a slot file, [`Refusal::NoValidSlot`] and [`Refusal::FatalSlot`],
//!   which the slot file alone causes and plain `repair` answers ([F16] P-61, P-85). A refusal that names no file names
//!   no declared file: the subject's own declaration explains only what it ties to that file (S4: the enumerator, not
//!   the subject, decides which refusals are correct).
//! - A file the setup put in place that the protocol must ignore ([`super::Ledger::setup_ignored`]: a stray extent of
//!   another epoch, a misplaced copy, which [F16] P-54 and P-55 keep out of every state) explains no refusal at all: a
//!   refusal concerning it shows that the store did not ignore it.
//!
//! The recovering writer's final answer may not be a refusal that the operator's repair answers: [`Refusal::NoValidSlot`]
//! and [`Refusal::FatalSlot`] (plain `repair` from the extent heads, [F16] P-61, P-85), and [`Refusal::Damaged`] for a
//! sealed file (`repair --rebuild-from-log`, [80 §2.5] rule 8; [F16] P-68). A recovery that answered such a refusal with
//! repair reports it in [`super::Recovered::answered`], where it is judged like any other.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use moirai_vfs::VfsErrorKind;

use crate::adversary::Site;
use crate::trace::{
    CAPTURE_OK, CLASS_SLOT, Event, EventKind, NOTE_CLASS, NOTE_PATH, SlotCapture, path_hash_of,
};
use crate::world::{CallKind, error_code};

/// Why an operation or a recovery refused ([F19] exit 7 and its codes; [F16 §17.2] avail).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// `HEAD` has no valid slot after the re-reads ([F16] P-61, [F04 §8.1]): `store_corrupt`, for `moirai repair`.
    NoValidSlot,
    /// A slot passes its checksum but fails a validity check ([F04 §7] checks 3–5, [F16] P-61): `store_corrupt`.
    FatalSlot,
    /// A store file that the selected state names is missing, of another size or content, or fails its identity check
    /// ([F16] P-59, P-68): `store_corrupt` or `sealed_size`, naming the file.
    Damaged(PathBuf),
    /// Another corruption: an invalid group below `durable_lsn` ([F16] P-58), a publish that would write a fatal slot
    /// ([F16] P-48), an extent longer than E ([F16] P-72).
    Corrupt,
    /// A read error the operation cannot get past ([F16] P-92: `store_io_fault`).
    IoFault,
    /// `DiskFull` on a write, flush, create or namespace operation (decision (f): `disk_full`).
    DiskFull,
    /// Another I/O error of a write, flush or namespace operation (`fail_stop`: `durability_failure`, [OS/fs §4.4.5]).
    Io,
    /// A lock-layer refusal ([OS/lock §4] `LockError`: `NoLockFile`, `IdentityMismatch`, `AccessDenied`, `Io`).
    Lock,
    /// No store where discovery looks ([F02 §3.2]).
    NotAStore,
    /// A group lost again after two re-runs ([F16] P-47: `outcome_unknown`).
    OutcomeUnknown,
    /// A bounded wait ran out: `store_locked` (P-27), `outcome_pending` (P-41), `maintenance_busy` (P-85), a quiet byte
    /// taken by every requester ([F03 §3.1] rule 4).
    Busy,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Refusal::NoValidSlot => write!(f, "no valid slot"),
            Refusal::FatalSlot => write!(f, "a fatal slot"),
            Refusal::Damaged(p) => write!(f, "damaged {}", p.display()),
            Refusal::Corrupt => write!(f, "corrupt"),
            Refusal::IoFault => write!(f, "an I/O fault"),
            Refusal::DiskFull => write!(f, "disk full"),
            Refusal::Io => write!(f, "an I/O error"),
            Refusal::Lock => write!(f, "a lock refusal"),
            Refusal::NotAStore => write!(f, "not a store"),
            Refusal::OutcomeUnknown => write!(f, "outcome unknown"),
            Refusal::Busy => write!(f, "a bounded wait ran out"),
        }
    }
}

/// A refusal with the subject's own text (the exit-7 message) and the files it concerns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refused {
    /// Why.
    pub reason: Refusal,
    /// The message.
    pub detail: String,
    /// The absolute paths of the files the refusing state names or references where it refused, beyond the one
    /// [`Refusal::Damaged`] names: the extent that holds an invalid group below `durable_lsn` ([F16] P-58), the slot
    /// file of a publish that would write a fatal slot (P-48), an extent longer than E (P-72). An FM-10 act explains the
    /// refusal only through these files (module documentation); empty: the refusal names none.
    pub files: Vec<PathBuf>,
}

impl Refused {
    /// A refusal for `reason` with message `detail`, naming no file beyond the one [`Refusal::Damaged`] names.
    pub fn new(reason: Refusal, detail: impl Into<String>) -> Refused {
        Refused {
            reason,
            detail: detail.into(),
            files: Vec::new(),
        }
    }

    /// The refusal, concerning also the file at the absolute `path` ([`Refused::files`]).
    #[must_use]
    pub fn concerning(mut self, path: impl Into<PathBuf>) -> Refused {
        self.files.push(path.into());
        self
    }

    /// The files the refusal concerns: the one [`Refusal::Damaged`] names, then [`Refused::files`].
    pub fn concerned(&self) -> impl Iterator<Item = &Path> {
        let damaged = match &self.reason {
            Refusal::Damaged(p) => Some(p.as_path()),
            _ => None,
        };
        damaged
            .into_iter()
            .chain(self.files.iter().map(PathBuf::as_path))
    }
}

impl core::fmt::Display for Refused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} ({})", self.reason, self.detail)?;
        for (i, p) in self.files.iter().enumerate() {
            let sep = if i == 0 { ", concerning " } else { ", " };
            write!(f, "{sep}{}", p.display())?;
        }
        Ok(())
    }
}

/// One file a recovery's diagnosis names as damaged (`doctor --fsck`, an exit-7 message).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnosis {
    /// The file.
    pub path: PathBuf,
    /// The recovered state references it (the segment set, a retired extent's `hist`, a pin): then a fault the run
    /// injected on it must explain the damage ([F15] G-13). A partial file nothing references — what a maintenance run
    /// that died before its `Checkpoint` leaves ([F16] P-78, P-79) — is not.
    pub referenced: bool,
}

impl Diagnosis {
    /// A referenced file named as damaged.
    pub fn referenced(path: impl Into<PathBuf>) -> Diagnosis {
        Diagnosis {
            path: path.into(),
            referenced: true,
        }
    }
}

/// The faults a run injected, read from its trace up to some event ([`Facts::feed`] is incremental, so a crash image's
/// facts are those of its prefix).
#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    /// System crashes.
    pub(crate) crashes: u64,
    /// File flushes that failed: a caller's (`FlushEnd` 1), a death's outcome (`InFlight` 1/1), an external actor's.
    pub(crate) failed_flushes: u64,
    /// A flush of a slot file failed (OP-1: both slots may then be invalid).
    pub(crate) slot_flush_failed: bool,
    /// A slot write failed or was cut and changed its slot's bytes; [`Facts::slot_cut_then_crash`] once a crash follows.
    slot_damaged: bool,
    /// A slot write that failed or was cut, then a system crash ([F04 §8.1]).
    pub(crate) slot_cut_then_crash: bool,
    /// A slot write that failed or was cut and covered both slots.
    pub(crate) slot_cut_both: bool,
    /// Without declared slot files: a write that failed or was cut, then a crash (any file may be the slot file).
    any_cut: bool,
    pub(crate) any_cut_then_crash: bool,
    /// Whether the world watched slot files ([`crate::Watch::slots`]): the slot facts above are exact.
    pub(crate) slots_watched: bool,
    /// The nodes an external actor acted on (FM-10).
    pub(crate) external: BTreeSet<u64>,
    /// An FM-10 act on a slot file.
    pub(crate) external_slot: bool,
    /// An external actor replaced a file (FM-10.3, `LOCK`).
    pub(crate) replaced: bool,
    /// The nodes a read of failed (FM-12), and whether a mapped read faulted (FM-9.1).
    pub(crate) read_failed: BTreeSet<u64>,
    pub(crate) read_fault: bool,
    /// The adversary failed a write, flush, `sync_dir`, create or namespace operation (FM-5, FM-3).
    pub(crate) io_fault: bool,
    /// The adversary started a sharing violation (FM-8.2), or an external actor held a file exclusively.
    pub(crate) sharing: bool,
    /// The latest class of every node ([`NOTE_CLASS`]).
    classes: std::collections::BTreeMap<u64, u64>,
    /// The latest path hash of every node a [`NOTE_PATH`] named, and the path hashes of the files an external act or a
    /// read error touched (a file a repair replaced since is still known by its path).
    paths: std::collections::BTreeMap<u64, u64>,
    external_paths: BTreeSet<u64>,
    read_paths: BTreeSet<u64>,
}

/// The `Return` codes of a fault: `DiskFull`, `InsufficientSpace`, `Io`, `FlushFailed` (FM-5, FM-12).
fn fault_code(c: u64) -> bool {
    [
        VfsErrorKind::DiskFull,
        VfsErrorKind::InsufficientSpace,
        VfsErrorKind::Io,
        VfsErrorKind::FlushFailed,
    ]
    .iter()
    .any(|&k| error_code(k) == c)
}

impl Facts {
    /// Facts of a world whose watch has (or has not) slot files.
    pub(crate) fn new(slots_watched: bool) -> Facts {
        Facts {
            slots_watched,
            ..Facts::default()
        }
    }

    fn is_slot(&self, node: u64) -> bool {
        self.classes.get(&node).is_some_and(|c| c & CLASS_SLOT != 0)
    }

    /// The class of `node` the last class note gave.
    pub(crate) fn class(&self, node: u64) -> u64 {
        self.classes.get(&node).copied().unwrap_or(0)
    }

    /// Whether an external act or a read error touched the file at `path`, under this node or an earlier one.
    pub(crate) fn faulted(&self, path: &Path, node: Option<u64>) -> bool {
        self.modified(path, node)
            || node.is_some_and(|n| self.read_failed.contains(&n))
            || self.read_paths.contains(&path_hash_of(path))
    }

    /// Whether an external act touched the file at `path`, under this node or an earlier one (FM-10: its bytes may
    /// differ from every write moirai made).
    pub(crate) fn modified(&self, path: &Path, node: Option<u64>) -> bool {
        node.is_some_and(|n| self.external.contains(&n))
            || self.external_paths.contains(&path_hash_of(path))
    }

    /// Records a fault on `node` by its latest path: an external act's, else a read error's.
    fn fault_path(&mut self, node: u64, external: bool) {
        if let Some(&h) = self.paths.get(&node) {
            if external {
                self.external_paths.insert(h);
            } else {
                self.read_paths.insert(h);
            }
        }
    }

    /// Takes in one event; `capture` resolves a slot-write note's capture.
    pub(crate) fn feed(&mut self, e: &Event, capture: Option<&SlotCapture>) {
        match e.kind {
            EventKind::Note if e.a == NOTE_CLASS => {
                if e.c == 0 {
                    self.classes.remove(&e.b);
                } else {
                    self.classes.insert(e.b, e.c);
                }
            }
            EventKind::Note if e.a == NOTE_PATH => {
                self.paths.insert(e.b, e.c);
            }
            EventKind::Choice | EventKind::Injected if e.c != 0 => {
                let site = e.a;
                let io = [
                    Site::WriteFault,
                    Site::FlushFault,
                    Site::SyncDirFault,
                    Site::CreateFault,
                    Site::NsFault,
                ];
                if io.iter().any(|&s| s as u64 == site) {
                    self.io_fault = true;
                } else if site == Site::Sharing as u64 {
                    self.sharing = true;
                } else if site == Site::ReadFault as u64 || site == Site::MapFault as u64 {
                    self.read_fault = true;
                }
            }
            EventKind::FlushEnd if e.b < 2 && e.c == 1 => {
                self.failed_flushes += 1;
                self.slot_flush_failed |= self.is_slot(e.a);
            }
            EventKind::InFlight if e.b == 1 && e.c == 1 => {
                self.failed_flushes += 1;
                self.slot_flush_failed |= self.is_slot(e.a);
            }
            EventKind::InFlight if e.b == 0 => {
                // A death's or crash's partial write; a slot write's damage comes from its capture.
                if e.c != crate::adversary::PartialWrite::Nothing.to_choice() {
                    self.any_cut = true;
                }
            }
            EventKind::Return if e.a == CallKind::Write as u64 && e.c != 0 => self.any_cut = true,
            EventKind::Return if e.c != 0 && fault_code(e.c) && e.a == CallKind::Read as u64 => {
                self.read_failed.insert(e.b);
                self.fault_path(e.b, false);
                self.read_fault = true;
            }
            EventKind::MapRead if e.c == 2 => {
                self.read_failed.insert(e.a);
                self.read_fault = true;
            }
            EventKind::External => {
                self.external.insert(e.a);
                self.fault_path(e.a, true);
                if e.b == 3 {
                    self.fault_path(e.c, true);
                }
                let slot = self.is_slot(e.a);
                match e.b {
                    // A flush: a failed one poisons (FM-3.1).
                    2 => {
                        if e.c != 0 {
                            self.failed_flushes += 1;
                            self.slot_flush_failed |= slot;
                        }
                    }
                    3 => {
                        self.replaced = true;
                        self.external.insert(e.c);
                        self.external_slot |= slot || self.is_slot(e.c);
                    }
                    4 => self.sharing = true,
                    5 | 6 => {
                        self.read_fault = true;
                        self.read_failed.insert(e.a);
                    }
                    _ => self.external_slot |= slot,
                }
            }
            EventKind::Crash => {
                self.crashes += 1;
                if self.slot_damaged {
                    self.slot_cut_then_crash = true;
                }
                if self.any_cut {
                    self.any_cut_then_crash = true;
                }
            }
            _ => {}
        }
        if let Some(c) = capture
            && c.status != CAPTURE_OK
        {
            let changed = |s: usize| c.before[s][..] != c.after[s][..];
            let hit = |s: u64| c.offset < (s + 1) * 4096 && c.offset + c.len > s * 4096;
            if changed(0) || changed(1) {
                self.slot_damaged = true;
            }
            if hit(0) && hit(1) && changed(0) && changed(1) {
                self.slot_cut_both = true;
            }
        }
    }
}

/// What the setup declared before the run ([`super::Ledger::setup_fault`], [`super::Ledger::setup_ignored`]), with the
/// subject's slot files ([`super::Subject::slot_files`]): all absolute paths, compared component by component.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Setup<'a> {
    /// The files the setup damaged (FM-10 acts before the run).
    pub(crate) faults: &'a [PathBuf],
    /// The files the setup put in place that the protocol must ignore.
    pub(crate) ignored: &'a [PathBuf],
    /// The slot files.
    pub(crate) slots: &'a [PathBuf],
}

/// Whether `path` is one of `paths`, compared by their `/`-separated forms ([`path_hash_of`]).
fn listed(paths: &[PathBuf], path: &Path) -> bool {
    let h = path_hash_of(path);
    paths.iter().any(|p| path_hash_of(p) == h)
}

impl Setup<'_> {
    /// Whether the protocol must ignore the file at `path`.
    pub(crate) fn ignores(&self, path: &Path) -> bool {
        listed(self.ignored, path)
    }

    /// Whether a setup fault (not an ignored file) names the file at `path`.
    pub(crate) fn damaged(&self, path: &Path) -> bool {
        listed(self.faults, path) && !self.ignores(path)
    }

    /// Whether a setup fault names a slot file.
    pub(crate) fn slot_fault(&self) -> bool {
        self.faults
            .iter()
            .any(|p| listed(self.slots, p) && !self.ignores(p))
    }

    /// Whether the setup damaged any file.
    fn any_fault(&self) -> bool {
        self.faults.iter().any(|p| !self.ignores(p))
    }
}

/// The context a refusal is judged in.
pub(crate) struct Allow<'a> {
    pub(crate) facts: &'a Facts,
    /// A byte that a dead process holds beyond every wait bound, or never releases, may keep a recovering process out
    /// (FM-8.1 classes (b), (c)).
    pub(crate) busy: bool,
    /// A workload operation (live holders and pauses make every bounded wait a possible answer, FM-6).
    pub(crate) workload: bool,
    /// The store must keep no operation (nothing acknowledged, nothing seen).
    pub(crate) nothing_required: bool,
    /// The setup's declarations and the slot files.
    pub(crate) setup: Setup<'a>,
}

impl Allow<'_> {
    /// Whether an FM-10 act explains a refusal that concerns `files` (module documentation): an act during the run on
    /// one of them, or on any file when `files` is empty; a setup fault on one of them; never a file the protocol must
    /// ignore.
    fn fm10(&self, files: &[&Path]) -> bool {
        let f = self.facts;
        if files.is_empty() {
            return !f.external.is_empty();
        }
        files
            .iter()
            .any(|p| !self.setup.ignores(p) && (f.modified(p, None) || self.setup.damaged(p)))
    }

    /// `Ok` if `r` is a correct answer here, else what it lacks.
    pub(crate) fn check(&self, r: &Refused) -> Result<(), String> {
        let f = self.facts;
        let files: Vec<&Path> = r.concerned().collect();
        let fm10 = self.fm10(&files);
        // Without declared slot files any FM-10 act may concern one.
        let fm10_any = !f.external.is_empty() || self.setup.any_fault();
        let ok = match &r.reason {
            Refusal::NoValidSlot => {
                let slot = f.slot_flush_failed
                    || f.slot_cut_then_crash
                    || f.slot_cut_both
                    || f.external_slot
                    || self.setup.slot_fault();
                // Without declared slot files any failed flush, or any cut write and a crash, may concern one.
                slot || (!f.slots_watched
                    && (f.failed_flushes > 0 || f.any_cut_then_crash || fm10_any))
            }
            Refusal::FatalSlot => {
                f.external_slot || self.setup.slot_fault() || (!f.slots_watched && fm10_any)
            }
            Refusal::Damaged(_) | Refusal::Corrupt => fm10 || f.read_fault || f.sharing,
            Refusal::IoFault => f.read_fault,
            Refusal::DiskFull | Refusal::Io => f.io_fault,
            Refusal::Lock => f.replaced || fm10,
            Refusal::NotAStore => self.nothing_required,
            Refusal::OutcomeUnknown => f.failed_flushes > 0 || f.read_fault,
            Refusal::Busy => self.workload || self.busy,
        };
        if ok {
            return Ok(());
        }
        let ignored: Vec<String> = files
            .iter()
            .filter(|p| self.setup.ignores(p))
            .map(|p| p.display().to_string())
            .collect();
        if !ignored.is_empty() {
            return Err(format!(
                "it concerns {}, a file the protocol must ignore: no refusal is a correct answer for it ([F16] P-54, \
                 P-55; [F15] FM-10)",
                ignored.join(", ")
            ));
        }
        Err(match &r.reason {
            Refusal::NoValidSlot => "no slot-file flush failed, no cut slot write was followed by a crash or covered both \
                 slots, and no external act or setup fault touched a slot file ([F16] P-61, [F04 §8.1], OP-1)"
                .to_owned(),
            Refusal::FatalSlot => {
                "no external act or setup fault touched a slot file: a fatal slot shows a defective writer ([F04 §7], \
                 [F16] P-48)"
                    .to_owned()
            }
            Refusal::Damaged(p) => format!(
                "no external act or setup fault on {}, read error or sharing violation explains it ([F16] P-59, \
                 P-68; [F15] G-13, FM-10)",
                p.display()
            ),
            Refusal::Corrupt if files.is_empty() => {
                "no external act during the run, read error or sharing violation explains it, and it names no file a \
                 setup fault could explain ([F16] P-58, P-48; [F15] G-13, FM-10)"
                    .to_owned()
            }
            Refusal::Corrupt => format!(
                "no external act or setup fault on {}, read error or sharing violation explains it ([F16] P-58, \
                 P-48; [F15] G-13, FM-10)",
                files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Refusal::IoFault => "no read failed ([F15] FM-12, [F16] P-92)".to_owned(),
            Refusal::DiskFull | Refusal::Io => {
                "no write, flush, create or namespace operation failed ([F15] FM-5, FM-3)".to_owned()
            }
            Refusal::Lock => {
                "LOCK was not replaced and no external act or setup fault on a file it concerns explains it ([F15] \
                 FM-10.3)"
                    .to_owned()
            }
            Refusal::NotAStore => "the store must keep an acknowledged or observed operation".to_owned(),
            Refusal::OutcomeUnknown => {
                "no flush and no read failed ([F16] P-47, [F16 §17.2] avail)".to_owned()
            }
            Refusal::Busy => "no dead process holds a byte beyond every wait bound ([F15] FM-8.1)".to_owned(),
        })
    }

    /// `Ok` if the recovering writer may end with `r` (after the operator's repair, where one exists), else why not.
    pub(crate) fn check_final(
        &self,
        r: &Refused,
        sealed: &dyn Fn(&Path) -> bool,
    ) -> Result<(), String> {
        match &r.reason {
            Refusal::NoValidSlot | Refusal::FatalSlot => Err(format!(
                "{}: the operator's plain repair rebuilds both slots from the extent heads ([F16] P-61, P-85)",
                r.reason
            )),
            Refusal::Damaged(p) if sealed(p) => Err(format!(
                "{}: the sealed file is derived, and repair rebuilds it from the log ([80 §2.5] rule 8, [F16] P-68)",
                r.reason
            )),
            _ => self.check(r),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::zero_page;
    use crate::trace::{CAPTURE_CRASHED, CAPTURE_DIED, CAPTURE_FAILED, CLASS_DISCOVERABLE};
    use std::sync::Arc;

    fn ev(kind: EventKind, a: u64, b: u64, c: u64) -> Event {
        Event {
            kind,
            task: 0,
            proc: 0,
            a,
            b,
            c,
        }
    }

    fn feed(events: &[Event]) -> Facts {
        let mut f = Facts::new(true);
        for e in events {
            f.feed(e, None);
        }
        f
    }

    const SLOT: u64 = 5;
    const LOG: u64 = 6;

    fn slot_class() -> Event {
        ev(
            EventKind::Note,
            NOTE_CLASS,
            SLOT,
            CLASS_SLOT | CLASS_DISCOVERABLE,
        )
    }

    fn allow(f: &Facts) -> Allow<'_> {
        Allow {
            facts: f,
            busy: false,
            workload: false,
            nothing_required: false,
            setup: Setup::default(),
        }
    }

    /// A refusal for `reason` that names no file beyond the one [`Refusal::Damaged`] names.
    fn r(reason: Refusal) -> Refused {
        Refused::new(reason, "exit 7")
    }

    /// A slot write's capture: `status`, the range, and whether it changed each slot.
    fn cut(status: u8, offset: u64, len: u64, changes: [bool; 2]) -> SlotCapture {
        let page = |b: u8| Arc::new([b; 4096]);
        SlotCapture {
            proc: 0,
            task: 0,
            node: SLOT,
            offset,
            len,
            status,
            before: [zero_page(), zero_page()],
            after: [
                if changes[0] { page(1) } else { zero_page() },
                if changes[1] { page(1) } else { zero_page() },
            ],
            poisoned: false,
            boot: None,
        }
    }

    #[test]
    fn no_valid_slot_needs_a_slot_flush_failure_or_a_cut_slot_write_and_a_crash() {
        // E7: a failed flush of a log extent alone does not explain it.
        let log_only = feed(&[
            slot_class(),
            ev(EventKind::FlushEnd, LOG, 0, 1),
            ev(EventKind::Crash, 0, 0, 0),
        ]);
        assert_eq!(log_only.failed_flushes, 1);
        assert!(allow(&log_only).check(&r(Refusal::NoValidSlot)).is_err());
        // A failed flush of the slot file does, by a caller or at a death.
        let slot = feed(&[slot_class(), ev(EventKind::FlushEnd, SLOT, 1, 1)]);
        assert!(allow(&slot).check(&r(Refusal::NoValidSlot)).is_ok());
        let death = feed(&[slot_class(), ev(EventKind::InFlight, SLOT, 1, 1)]);
        assert!(allow(&death).check(&r(Refusal::NoValidSlot)).is_ok());
        // A cut slot write needs a crash after it, unless it covered both slots.
        for status in [CAPTURE_FAILED, CAPTURE_DIED, CAPTURE_CRASHED] {
            let mut f = Facts::new(true);
            f.feed(&slot_class(), None);
            let c = cut(status, 4096, 4096, [false, true]);
            f.feed(&ev(EventKind::Note, 0, SLOT, 0), Some(&c));
            assert!(allow(&f).check(&r(Refusal::NoValidSlot)).is_err());
            f.feed(&ev(EventKind::Crash, 0, 0, 0), None);
            assert!(allow(&f).check(&r(Refusal::NoValidSlot)).is_ok());
        }
        let mut both = Facts::new(true);
        let c = cut(CAPTURE_DIED, 0, 8192, [true, true]);
        both.feed(&ev(EventKind::Note, 0, SLOT, 0), Some(&c));
        assert!(allow(&both).check(&r(Refusal::NoValidSlot)).is_ok());
        // A cut that changed nothing, or a successful write, is no damage.
        let mut none = Facts::new(true);
        let c = cut(CAPTURE_DIED, 0, 4096, [false, false]);
        none.feed(&ev(EventKind::Note, 0, SLOT, 0), Some(&c));
        none.feed(&ev(EventKind::Crash, 0, 0, 0), None);
        assert!(allow(&none).check(&r(Refusal::NoValidSlot)).is_err());
        // An external act on the slot file explains it, and a fatal slot too.
        let ext = feed(&[slot_class(), ev(EventKind::External, SLOT, 1, 0)]);
        assert!(allow(&ext).check(&r(Refusal::NoValidSlot)).is_ok());
        assert!(allow(&ext).check(&r(Refusal::FatalSlot)).is_ok());
        assert!(allow(&log_only).check(&r(Refusal::FatalSlot)).is_err());
        // Without declared slot files any failed flush counts.
        let mut blind = Facts::new(false);
        blind.feed(&ev(EventKind::FlushEnd, LOG, 0, 1), None);
        assert!(allow(&blind).check(&r(Refusal::NoValidSlot)).is_ok());
    }

    #[test]
    fn every_reason_needs_its_fault() {
        let clean = feed(&[]);
        let a = allow(&clean);
        for x in [
            Refusal::Damaged(PathBuf::from("/s/seg.1")),
            Refusal::Corrupt,
            Refusal::IoFault,
            Refusal::DiskFull,
            Refusal::Io,
            Refusal::Lock,
            Refusal::NotAStore,
            Refusal::OutcomeUnknown,
            Refusal::Busy,
        ] {
            assert!(
                a.check(&Refused::new(x.clone(), "")).is_err(),
                "{x} in a clean run"
            );
        }
        // E5: a read fault explains IoFault, Damaged, Corrupt and OutcomeUnknown, not DiskFull.
        let read = feed(&[ev(EventKind::Choice, Site::ReadFault as u64, 3, 1)]);
        let a = allow(&read);
        assert!(a.check(&r(Refusal::IoFault)).is_ok());
        assert!(a.check(&r(Refusal::Corrupt)).is_ok());
        assert!(a.check(&r(Refusal::Damaged(PathBuf::from("/s/x")))).is_ok());
        assert!(a.check(&r(Refusal::OutcomeUnknown)).is_ok());
        assert!(a.check(&r(Refusal::DiskFull)).is_err());
        // A disk-full write explains DiskFull and Io, not Corrupt.
        let full = feed(&[ev(EventKind::Choice, Site::WriteFault as u64, 3, 1)]);
        assert!(allow(&full).check(&r(Refusal::DiskFull)).is_ok());
        assert!(allow(&full).check(&r(Refusal::Corrupt)).is_err());
        // E13: a failed flush explains OutcomeUnknown; a flush holder re-writes before it flushes, so not Corrupt.
        let ff = feed(&[ev(EventKind::FlushEnd, LOG, 0, 1)]);
        assert!(allow(&ff).check(&r(Refusal::OutcomeUnknown)).is_ok());
        assert!(allow(&ff).check(&r(Refusal::Corrupt)).is_err());
        assert!(allow(&ff).check(&r(Refusal::IoFault)).is_err());
        // A replaced LOCK explains a lock refusal.
        let rep = feed(&[ev(EventKind::External, 9, 3, 10)]);
        assert!(allow(&rep).check(&r(Refusal::Lock)).is_ok());
        // A declared setup fault is an FM-10 act on its file only: see
        // `an_fm10_act_explains_a_refusal_only_through_its_file`.
        // NotAStore only while nothing must survive.
        let a = Allow {
            nothing_required: true,
            ..allow(&clean)
        };
        assert!(a.check(&r(Refusal::NotAStore)).is_ok());
    }

    /// E6: a file an external act truncated, or a read error hit, stays faulted by its path after a repair replaced it
    /// with a new node; another file does not.
    #[test]
    fn a_fault_is_known_by_its_path_after_a_repair_replaced_the_file() {
        let seg = std::path::Path::new("/s/seg.1");
        let h = crate::trace::path_hash("/s/seg.1");
        let f = feed(&[
            ev(EventKind::Note, crate::trace::NOTE_PATH, 7, h),
            ev(EventKind::External, 7, 0, 100),
        ]);
        assert!(f.faulted(seg, Some(7)) && f.faulted(seg, Some(8)) && f.faulted(seg, None));
        assert!(f.modified(seg, Some(8)));
        assert!(!f.faulted(std::path::Path::new("/s/seg.2"), Some(8)));
        // A read error's file, likewise, though its bytes are not modified.
        let rd = feed(&[
            ev(EventKind::Note, crate::trace::NOTE_PATH, 7, h),
            ev(
                EventKind::Return,
                CallKind::Read as u64,
                7,
                error_code(VfsErrorKind::Io),
            ),
        ]);
        assert!(rd.faulted(seg, Some(8)) && !rd.modified(seg, Some(8)));
        assert!(allow(&rd).check(&r(Refusal::IoFault)).is_ok());
    }

    #[test]
    fn bounded_waits_are_answers_for_live_contention_and_dead_holders_only() {
        let clean = feed(&[]);
        // A workload operation may time out behind live holders (FM-6).
        let w = Allow {
            workload: true,
            ..allow(&clean)
        };
        assert!(w.check(&r(Refusal::Busy)).is_ok());
        // E12: the recovery (first read and writer) behind a byte held beyond every bound.
        let b = Allow {
            busy: true,
            ..allow(&clean)
        };
        assert!(b.check(&r(Refusal::Busy)).is_ok());
        assert!(
            b.check(&r(Refusal::Corrupt)).is_err(),
            "busy allows bounded waits only"
        );
        assert!(allow(&clean).check(&r(Refusal::Busy)).is_err());
    }

    #[test]
    fn the_writers_final_answer_may_not_be_a_refusal_that_repair_answers() {
        let slot = feed(&[slot_class(), ev(EventKind::FlushEnd, SLOT, 1, 1)]);
        let a = allow(&slot);
        let sealed = |p: &Path| p == Path::new("/s/seg.1");
        assert!(a.check(&r(Refusal::NoValidSlot)).is_ok());
        assert!(a.check_final(&r(Refusal::NoValidSlot), &sealed).is_err());
        let ext = feed(&[
            ev(
                EventKind::Note,
                NOTE_PATH,
                3,
                crate::trace::path_hash("/s/log.1"),
            ),
            ev(EventKind::External, 3, 0, 0),
        ]);
        let a = allow(&ext);
        assert!(
            a.check_final(&r(Refusal::Damaged(PathBuf::from("/s/seg.1"))), &sealed)
                .is_err()
        );
        assert!(
            a.check_final(&r(Refusal::Damaged(PathBuf::from("/s/log.1"))), &sealed)
                .is_ok()
        );
        let read = feed(&[ev(EventKind::Choice, Site::ReadFault as u64, 3, 2)]);
        assert!(
            allow(&read)
                .check_final(&r(Refusal::IoFault), &sealed)
                .is_ok()
        );
    }

    /// FM-10 (WP-40 closure, P-55 missed): a setup fault explains only a refusal that concerns its file, and on a slot
    /// file the slot refusals; a refusal that names no file is explained by no setup fault; a file the protocol must
    /// ignore explains nothing; an act during the run explains a refusal that concerns its file, or names none.
    #[test]
    fn an_fm10_act_explains_a_refusal_only_through_its_file() {
        let clean = feed(&[]);
        let head = PathBuf::from("/s/HEAD");
        let (log1, log2) = (PathBuf::from("/s/log.1"), PathBuf::from("/s/log.2"));
        let slots = [head.clone()];
        // A fatal HEAD fixture and a damaged log.1.
        let faults = [head.clone(), log1.clone()];
        let a = Allow {
            setup: Setup {
                faults: &faults,
                ignored: &[],
                slots: &slots,
            },
            ..allow(&clean)
        };
        assert!(a.check(&r(Refusal::FatalSlot)).is_ok());
        assert!(a.check(&r(Refusal::NoValidSlot)).is_ok());
        assert!(a.check(&r(Refusal::Corrupt).concerning(&log1)).is_ok());
        assert!(a.check(&r(Refusal::Damaged(log1.clone()))).is_ok());
        // The same path written with the host's separator.
        let native: PathBuf = ["/", "s", "log.1"].iter().collect();
        assert!(a.check(&r(Refusal::Damaged(native))).is_ok());
        assert!(a.check(&r(Refusal::Corrupt).concerning(&head)).is_ok());
        // Another file, or no file at all, is not explained by a declared fault.
        let m = a.check(&r(Refusal::Corrupt).concerning(&log2)).unwrap_err();
        assert!(m.contains("/s/log.2"), "{m}");
        assert!(a.check(&r(Refusal::Damaged(log2.clone()))).is_err());
        let m = a.check(&r(Refusal::Corrupt)).unwrap_err();
        assert!(m.contains("names no file"), "{m}");
        assert!(a.check(&r(Refusal::Lock)).is_err());
        // A file the protocol must ignore (P-55's foreign extent) explains nothing, even if also declared damaged.
        let ignored = [log1.clone()];
        let p55 = Allow {
            setup: Setup {
                faults: &faults,
                ignored: &ignored,
                slots: &slots,
            },
            ..allow(&clean)
        };
        let m = p55
            .check(&r(Refusal::Corrupt).concerning(&log1))
            .unwrap_err();
        assert!(m.contains("must ignore"), "{m}");
        assert!(p55.check(&r(Refusal::Damaged(log1.clone()))).is_err());
        assert!(p55.check(&r(Refusal::Corrupt)).is_err());
        // The slot fixture still explains the slot refusals that plain repair answers.
        assert!(p55.check(&r(Refusal::FatalSlot)).is_ok());
        let a = Allow {
            setup: Setup {
                faults: &[],
                ignored: &ignored,
                slots: &slots,
            },
            ..allow(&clean)
        };
        assert!(a.check(&r(Refusal::FatalSlot)).is_err());
        // A damaged non-slot file explains no slot refusal while the slot files are declared.
        let damaged = [log2.clone()];
        let a = Allow {
            setup: Setup {
                faults: &damaged,
                ignored: &[],
                slots: &slots,
            },
            ..allow(&clean)
        };
        assert!(a.check(&r(Refusal::FatalSlot)).is_err());
        assert!(a.check(&r(Refusal::NoValidSlot)).is_err());
        // An act during the run: through its file by path, or for a refusal that names none.
        let h = crate::trace::path_hash("/s/log.2");
        let ext = feed(&[
            ev(EventKind::Note, NOTE_PATH, 7, h),
            ev(EventKind::External, 7, 1, 0),
        ]);
        let a = allow(&ext);
        assert!(a.check(&r(Refusal::Corrupt).concerning(&log2)).is_ok());
        assert!(a.check(&r(Refusal::Corrupt)).is_ok());
        assert!(a.check(&r(Refusal::Corrupt).concerning(&log1)).is_err());
        assert!(a.check(&r(Refusal::Damaged(log1.clone()))).is_err());
        // A read error or a sharing violation explains it whatever it names.
        let read = feed(&[ev(EventKind::Choice, Site::ReadFault as u64, 3, 1)]);
        assert!(
            allow(&read)
                .check(&r(Refusal::Corrupt).concerning(&log1))
                .is_ok()
        );
        // The refusal shows the files it concerns.
        let shown = r(Refusal::Corrupt)
            .concerning(&log1)
            .concerning(&log2)
            .to_string();
        assert_eq!(shown, "corrupt (exit 7), concerning /s/log.1, /s/log.2");
    }
}
