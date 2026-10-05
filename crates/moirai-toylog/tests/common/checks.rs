//! The subject's own part of the harness (WP-40, E4) beyond the scenarios and the effects a state shows (`mod.rs`): the
//! tap that turns the toy's protocol notes into the crash enumerator's notes and namespace expectations ([`SimTap`]),
//! the read check of a reader's view against the operations acknowledged before it began ([`ReadWatch`]), the check of
//! a view a process kept against the replay of the valid log up to the view's bound ([`KeptWatch`], [F16] P-56), and
//! the reads of the project directories that the effect sets and the prefill use ([`ns_location`], [`read_all`]).
//!
//! **Authorship (PLAN §3.1 S4; [F13 §1.4], [F16 §17.2] "Where the detectors live for the toy vehicle").** S4 keeps the
//! seeded-bug author (R-TOY) from being the enumerator's author (R-HARN-S), so that the harness cannot be tuned to its
//! own bugs. The generic detectors are the enumerator's (`moirai_vfs_sim::enumerate`): the trace predicates (I-G4 and
//! I-G6 over the lock, flush and namespace events and the decoded `HEAD` slot writes, [F03 §3.1] rule 2 over the probe
//! rounds), the avail verdict over the typed refusals the subject reports, the chain verdict over the identity bytes of
//! acknowledged groups, fresh over observations with their group's position, and ns by node identity. The subject feeds
//! them: its ledger calls, the slot decoder and the notes below. What stays the toy's, by R-HARN-S's S4 disposition, is
//! the effects mapping, the scenarios and fixtures, `doctor --verify` (`moirai_toylog::verify`,
//! `Toy::head_fold_problem`, `Toy::missing_pinned_files`), [`ReadWatch`] and [`KeptWatch`]; a change to either after a
//! missed bug cites a fault-model item, as S4 asks of the enumerator. A toy check judges a rule of [F16] over the record
//! forms the specification defines, and never reports a record form, flag or value the toy adds only so that a seeded
//! bug can be expressed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use moirai_toylog::{Note, Replay, State, StoreFile, Tap, View, file_name};
use moirai_vfs::{Access, OpenHint, RelPath, RootAccess, RootRole, StoreFs};
use moirai_vfs_sim::enumerate::{
    EffectKey, EffectKind, MAINT_AUTOMATIC, MAINT_BELOW_CAP, NOTE_MAINT_DECISION, NOTE_PHASE,
};
use moirai_vfs_sim::{CallKind, EventKind, SECTOR, SimVfs, SimWorld};

use super::{BAD, DONE, OPEN, PROJ, STORE, Sink, TRASH, at_dir, content, lock};

// ---------------------------------------------------------------------------------------------------------------------
// The protocol notes ([F13 §1.4])

/// The `file rm --trash` operations of a task whose intent is not acknowledged yet: idempotency key → the file's source
/// path. The runner registers one before the operation; the tap takes it when the intent is acknowledged.
pub type TrashPending = Arc<Mutex<BTreeMap<u64, PathBuf>>>;

/// The tap of one process on the simulator. Phase 1 and phase 3 become [`NOTE_PHASE`] notes and a maintenance decision a
/// [`NOTE_MAINT_DECISION`] note, written through the process's `Vfs`, so each carries the task that made it
/// (`moirai_vfs_sim::enumerate` module `protocol`: P-25, P-51, [F03 §3.1] rule 2). For a workload task, an acknowledged
/// intent of a `file rm --trash` the runner registered records the namespace expectations that need the intent's id —
/// where its trash entry `trash/<lsn>/0` ([F16 §13.1]) must hold the file — in the run's ledger ([F16 §17.2] ns, [40
/// §3.4]). The store files the process uses ([`Note::Uses`]) are collected for the files its refusals concern
/// ([`SimTap::used`]); the clones of a tap share them.
#[derive(Clone)]
pub struct SimTap {
    vfs: SimVfs,
    world: SimWorld,
    trash: Option<(Sink, TrashPending)>,
    used: Arc<Mutex<BTreeSet<StoreFile>>>,
}

impl SimTap {
    /// The tap of a process that reports no namespace expectation (the recovery's processes).
    pub fn new(world: &SimWorld, vfs: &SimVfs) -> SimTap {
        SimTap {
            vfs: vfs.clone(),
            world: world.clone(),
            trash: None,
            used: Arc::default(),
        }
    }

    /// The tap of a workload task whose `file rm --trash` operations are registered in `pending` and whose expectations
    /// go to `sink`.
    pub fn with_ledger(
        world: &SimWorld,
        vfs: &SimVfs,
        sink: &Sink,
        pending: &TrashPending,
    ) -> SimTap {
        SimTap {
            vfs: vfs.clone(),
            world: world.clone(),
            trash: Some((sink.clone(), Arc::clone(pending))),
            used: Arc::default(),
        }
    }

    /// Forgets the store files used so far: a refusal from now on concerns the files the process uses from now on.
    pub fn clear_used(&self) {
        lock(&self.used).clear();
    }

    /// The absolute paths of the store files the process used since the last [`SimTap::clear_used`] ([`Note::Uses`]).
    pub fn used(&self) -> Vec<PathBuf> {
        lock(&self.used)
            .iter()
            .map(|f| Path::new(STORE).join(f.name().as_str()))
            .collect()
    }
}

impl Tap for SimTap {
    fn note(&self, n: Note) {
        match n {
            Note::Phase1(true) => self.vfs.note(NOTE_PHASE, 1, 0),
            Note::Phase3(true) => self.vfs.note(NOTE_PHASE, 3, 0),
            Note::Phase1(false) | Note::Phase3(false) => {}
            Note::Uses(f) => {
                lock(&self.used).insert(f);
            }
            Note::MaintenanceDecided {
                automatic,
                below_cap,
            } => {
                let flags = if automatic { MAINT_AUTOMATIC } else { 0 }
                    | if below_cap { MAINT_BELOW_CAP } else { 0 };
                self.vfs.note(NOTE_MAINT_DECISION, flags, 0);
            }
            Note::IntentAcked { key, lsn } => {
                let Some((sink, pending)) = &self.trash else {
                    return;
                };
                let Some(src) = lock(pending).remove(&key) else {
                    return;
                };
                // A file the operation already moved (a rename issued before the intent's identity check, [F16] P-16)
                // has no name at its source: the expectations recorded before the operation judge that state.
                if !self.world.exists(&src) {
                    return;
                }
                let entry = Path::new(STORE).join(format!("trash/{lsn}/0"));
                let open = [src.clone(), entry.clone()];
                let done = [entry];
                sink.expect_names(
                    &src,
                    EffectKey::new(EffectKind::Intent, key),
                    &[(Some(OPEN), &open[..]), (Some(DONE), &done[..])],
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Read visibility against the operations acknowledged before a view began

/// The node of the store's `HEAD`, if it exists.
fn head_node(w: &SimWorld) -> Option<u64> {
    w.node_at(&Path::new(STORE).join("HEAD"))
}

/// Whether a read of `HEAD` may return an older slot, or a mix of versions, because a flush of `HEAD` failed ([F15]
/// FM-3.1, FM-3.2: poisoning is per file, so a failed flush of a log extent never reaches `HEAD`). The poisoning ends
/// once both slots have been re-written and a flush of `HEAD` has then succeeded (FM-3.5): two successful writes of
/// `HEAD` (two publishes write the two slots) followed by a successful flush of it. Without the trace (a prefill), any
/// failed flush counts.
pub fn head_flush_failed(w: &SimWorld) -> bool {
    if w.failed_flushes() == 0 {
        return false;
    }
    let events = w.trace();
    if events.is_empty() {
        return true;
    }
    let Some(head) = head_node(w) else {
        return true;
    };
    let write = CallKind::Write as u64;
    let (mut poisoned, mut rewrites) = (false, 0u32);
    for e in &events {
        match e.kind {
            EventKind::FlushEnd if e.a == head && e.c == 1 => (poisoned, rewrites) = (true, 0),
            EventKind::InFlight if e.a == head && e.b == 1 && e.c == 1 => {
                (poisoned, rewrites) = (true, 0);
            }
            EventKind::Return if poisoned && e.a == write && e.b == head && e.c == 0 => {
                rewrites += 1;
            }
            EventKind::FlushEnd if poisoned && e.a == head && e.c == 0 && rewrites >= 2 => {
                poisoned = false;
            }
            _ => {}
        }
    }
    poisoned
}

/// Whether a scripted system crash happened in the run and no log flush has succeeded since ([OS/proc §5] U5, U6: an
/// Unknown-boot reader runs no boot-change recovery, and after a crash it sees the acknowledged commits only once the
/// next writer's flush has published them).
pub fn crash_not_caught_up(w: &SimWorld) -> bool {
    let events = w.trace();
    let Some(i) = events.iter().rposition(|e| e.kind == EventKind::Crash) else {
        return false;
    };
    !events[i..]
        .iter()
        .any(|e| e.kind == EventKind::FlushEnd && e.b == 0 && e.c == 0)
}

/// What one reader view ([F16 §8]) must show, taken when the view begins: the commits and the lazy rows acknowledged so
/// far, and the flush failures up to then.
pub struct ReadWatch {
    before: BTreeSet<u64>,
    lazy: BTreeMap<u64, u64>,
    failed_flushes: u64,
    head_ok: bool,
}

impl ReadWatch {
    /// The watch of a view that begins now, after the acknowledgement of the commits `before` and the lazy rows `lazy`
    /// (key, value). `unknown_boot`: the reader is in Unknown-boot mode ([OS/proc §5]).
    pub fn begin(
        w: &SimWorld,
        before: BTreeSet<u64>,
        lazy: BTreeMap<u64, u64>,
        unknown_boot: bool,
    ) -> ReadWatch {
        ReadWatch {
            before,
            lazy,
            failed_flushes: w.failed_flushes(),
            head_ok: !head_flush_failed(w) && !(unknown_boot && crash_not_caught_up(w)),
        }
    }

    /// The view `st` against the watch; `whole`: the view reached the slot's `committed_lsn`.
    ///
    /// Every commit acknowledged before the view began is in it (I-G2). An acknowledged commit lies below `durable_lsn`
    /// of every slot published after its acknowledgement, in sectors a failed log flush never poisons (FM-3.1 reaches
    /// only sectors dirty during the flush); only a failed `HEAD` flush can make a read return an older slot (FM-3.2,
    /// OP-1), and the check is not made after one — nor for an Unknown-boot reader after a scripted crash until the next
    /// flush ([OS/proc §5] U5, U6; [F13 §3.8] limits post-crash completeness to Known-boot readers).
    ///
    /// A lazy effect is reported as published, which promises visibility ([F16 §1.2] "acknowledge"), until a failed
    /// flush may take it back (FM-3.6). The view ends below `committed_lsn` only where the reader rules end it: a lazy
    /// tail lost after a crash or a failed flush, or a failed read at or above `durable_lsn` ([F16] P-58, P-92) — a view
    /// the lazy rows need not reach.
    pub fn judge(&self, w: &SimWorld, st: &State, whole: bool) -> Vec<String> {
        let mut out = Vec::new();
        if self.head_ok && !head_flush_failed(w) {
            for op in &self.before {
                if !st.commits.contains_key(op) {
                    out.push(format!(
                        "read freshness (I-G2): a view that began after the acknowledgement of commit {op:#x} does not \
                         show it"
                    ));
                }
            }
        }
        if whole && self.failed_flushes == 0 && w.failed_flushes() == 0 {
            for (k, v) in &self.lazy {
                if st.runtime.get(k) != Some(v) {
                    out.push(format!(
                        "visibility: a view that began after the acknowledgement of lazy row {k:#x} does not show it"
                    ));
                }
            }
        }
        out
    }
}

/// The check of a view a process kept ([F16] P-56, [80 §2.4.3] "Readers"), taken when the operation that refreshes it
/// begins: after the re-check of its bound, the process's view equals the replay of the valid log from the selected
/// slot's segment set up to the view's bound. The toy's own model of that replay is a reader that starts after the
/// refresh and replays the newest slot's set and the log up to exactly the view's bound
/// ([`moirai_toylog::Toy::replay_to`]); [`KeptWatch::judge`] compares the two.
///
/// It runs after every refresh of a reader that kept a view from an earlier read, and after every acknowledged
/// operation of a writer that kept its view (the view of its phase 1 and phase 2a, [F16] P-25, P-30); a writer's
/// operation that ends in a refusal is not judged.
///
/// Its fault-model items (S4: it was added after the review of WP-40 found P-56 reported only through the ledger's ack
/// and fresh verdicts, made an equality after R-HARN-S's S4 review, and made to judge a bound that the valid log no
/// longer has after that review's second round, which found P-56 detected on one seed of 24):
/// - **P-56's own terms:** a process re-checks its bound only when it reads a slot with a new `slot_seq`; under the
///   slot it last read it keeps its view, which a lazy tail lost after a failed flush may have made stale ([F16] P-58,
///   FM-3.6), also where no sector reads as poisoned any more: an append into a poisoned sector fixes the sector's
///   other sub-sectors, a lost tail among them, to a version of their candidate set (FM-3.5). Nothing is judged unless
///   the act's refresh read a new slot (the view's `slot_seq` changed). Under a new slot the re-check holds even then:
///   a writer appends below a bound a published slot covers only after it has published the lowered `committed_lsn`
///   ([F16] P-49), so the new slot's `committed_lsn` lies below the bound, or the refill re-wrote the 8 bytes before it.
///   A read of `HEAD` that returns an older slot (FM-3.2, OP-1) changes no byte of the log, so its re-check is judged
///   like any other.
/// - **FM-3.6:** a flush that fails between the refresh and the replay may take back a lazy tail, and a writer's next
///   flush re-writes the pending range (FM-3.5): nothing is judged after a failed flush since the watch began.
///   Without one, and without a crash (which ends the process), the bytes below the view's bound cannot change between
///   the refresh and the replay: appends land at the end of the valid log, and the protocol writes below a bound a
///   view reached only to re-write a pending range or to refill a lost tail, after a failed flush or a crash, and only
///   after a publish that lowers `committed_lsn` below the view's bound ([F16] P-43, P-49), which the re-check drops.
/// - **FM-3.2:** a sector that a failed flush poisoned before the watch began, and that nobody has written since, may
///   read a different version on every read, so the refresh and the replay may read different valid group sequences
///   there. Nothing is judged when a log extent from the view's base on has a poisoned sector below the view's bound,
///   asked of the simulator ([`SimWorld::poisoned_below`]) when the watch begins and again when it judges. A sector is
///   poisoned only by a failed flush (FM-3.1), which the guard above excludes after the watch began, so a sector that a
///   read between the two questions may have drawn from its candidate set is poisoned at the first. The lower bound is
///   the view's base at the watch's start: every read the two views rest on starts at or above it (the refresh's from
///   the kept bound or from a newer set's bound, the replay's from the newest set's, the newest set's own from the set
///   before it), and a view rests on its own earlier reads only through the chain value at its bound, which the
///   refresh re-checks.
/// - **FM-12, FM-10:** a failed read, and an extent that an external truncation shortened, end the replay's scan
///   without saying anything about the log below the bound ([`Replay::Unreached`]); a truncation never makes an
///   invalid group, since a scan checks an extent's length before it reads it ([F05 §5.2] check 1).
/// - With those excluded, the view's bound is the end of a valid group the refresh read, and the bytes below it are
///   what the replay reads, so the replay reaches the bound and shows what the view shows. A replay that finds a valid
///   group spanning the bound ([`Replay::Spanned`]), or that ends below it with an invalid group ([`Replay::Short`]),
///   shows a bound the valid log no longer has: the view kept a lost tail, refilled with other group boundaries or not
///   at all. A replay whose set folds beyond the bound, whose newest slot is of another epoch, or that stops below the
///   bound at a failed read or at the end of the extents ([`Replay::Unreached`]) judges nothing, nor does a replay that
///   refuses: its refusals are not the process's.
pub struct KeptWatch {
    failed_flushes: u64,
    /// The view's `slot_seq` when the watch began.
    slot_seq: u64,
    /// The view's base when the watch began: the lowest lsn the poison questions cover.
    from: u64,
    /// The lowest poisoned lsn of the log from `from`'s extent on when the watch began ([`lowest_poisoned`]).
    poisoned: u64,
    /// The extent length E.
    e: u64,
}

impl KeptWatch {
    /// The watch of an operation that begins now and refreshes the view `v` the process kept, in a store whose extents
    /// are `e` bytes long.
    pub fn begin(w: &SimWorld, v: &View, e: u64) -> KeptWatch {
        KeptWatch {
            failed_flushes: w.failed_flushes(),
            slot_seq: v.slot_seq,
            from: v.base,
            poisoned: lowest_poisoned(w, v.base, e),
            e,
        }
    }

    /// The view `v` of `who`, after its refresh, against `replay`, what a reader that starts now replays up to the view's
    /// bound from the newest segment set. When the replay reaches the bound, every commit (by key) and every lazy runtime
    /// row (key and value) the one holds, the other holds too: a kept overlay violates P-56 when it shows a row of a lost
    /// tail, shows an older value of a row the refill rewrote, or lacks a row the refill wrote below its bound. When a
    /// valid group spans the bound, or the valid log ends below it, the view kept a bound the valid log no longer has.
    pub fn judge(&self, w: &SimWorld, v: &View, replay: &Replay, who: &str) -> Vec<String> {
        let l0 = v.l0;
        if v.slot_seq == self.slot_seq
            || w.failed_flushes() != self.failed_flushes
            || l0 > self.poisoned
            || l0 > lowest_poisoned(w, self.from, self.e)
        {
            return Vec::new();
        }
        let replay = match replay {
            Replay::Reached(r) => r,
            Replay::Spanned { start, end } => {
                return vec![format!(
                    "kept view (P-56): {who} has the bound {l0}, which is no group boundary of the valid log: a valid \
                     group spans [{start}, {end})"
                )];
            }
            Replay::Short { end } => {
                return vec![format!(
                    "kept view (P-56): {who} has the bound {l0}, beyond the end of the valid log: the replay from the \
                     newest segment set finds an invalid group at {end}"
                )];
            }
            Replay::Unreached => return Vec::new(),
        };
        let st = &v.state;
        let model = format!(
            "the replay of the valid log from the newest segment set up to the view's bound {l0}"
        );
        let mut out = Vec::new();
        let mut report = |what: String| out.push(format!("kept view (P-56): {who} {what}"));
        for op in st
            .commits
            .keys()
            .filter(|op| !replay.commits.contains_key(op))
        {
            report(format!("shows commit {op:#x}, which {model} lacks"));
        }
        for op in replay
            .commits
            .keys()
            .filter(|op| !st.commits.contains_key(op))
        {
            report(format!("lacks commit {op:#x}, which {model} holds"));
        }
        for (k, v) in &st.runtime {
            match replay.runtime.get(k) {
                None => report(format!(
                    "shows lazy row {k:#x} = {v:#x}, which {model} lacks"
                )),
                Some(x) if x != v => report(format!(
                    "shows lazy row {k:#x} = {v:#x}, where {model} holds {x:#x}"
                )),
                Some(_) => {}
            }
        }
        for (k, x) in &replay.runtime {
            if !st.runtime.contains_key(k) {
                report(format!(
                    "lacks lazy row {k:#x} = {x:#x}, which {model} holds"
                ));
            }
        }
        out
    }
}

/// The lowest lsn of the store's log, at or above the first byte of the extent that holds `from`, whose sector is
/// poisoned now ([F15] FM-3.2: a read of it may return another version each time, [`SimWorld::poisoned_below`]), or
/// `u64::MAX` when none is. It asks of `log.<n>` from that extent on, up to the first extent that does not exist: the
/// extents of an epoch are numbered without gaps, and the ones below the newest set's bound that a checkpoint retired
/// are gone. A poisoned sector below `from` in the same extent counts too: the question is per file prefix.
pub fn lowest_poisoned(w: &SimWorld, from: u64, e: u64) -> u64 {
    let sectors = e.div_ceil(SECTOR);
    let mut n = from / e + 1;
    loop {
        let path = Path::new(STORE).join(format!("log.{n}"));
        if w.node_at(&path).is_none() {
            return u64::MAX;
        }
        if w.poisoned_below(&path, e) {
            // The least k with a poisoned sector among the first k: sector k − 1 is poisoned, none before it.
            let (mut clean, mut hit) = (0, sectors);
            while hit - clean > 1 {
                let mid = clean + (hit - clean) / 2;
                if w.poisoned_below(&path, mid * SECTOR) {
                    hit = mid;
                } else {
                    clean = mid;
                }
            }
            return (n - 1) * e + (hit - 1) * SECTOR;
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Where a project file is

/// Where project file `f` is now, as seen through `v` — the value of its namespace register in an effect set
/// (`mod.rs` `effects`): `at_dir(i)` with its content in directory i alone, `TRASH`, `BAD` (two places or other content),
/// or none. A file whose name exists but whose bytes cannot be read (an injected read error, [F15] FM-12) counts as
/// present with its content: the register names places, and content only where it is readable.
pub fn ns_location(v: &SimVfs, f: u64) -> Option<u64> {
    let name = file_name(f);
    let want = content(f);
    let mut found: Vec<u64> = Vec::new();
    let mut bad = false;
    let mut judge = |at: u64, got: Found| match got {
        Found::Absent => {}
        Found::Unreadable => found.push(at),
        Found::Bytes(b) if b == want => found.push(at),
        Found::Bytes(_) => bad = true,
    };
    for (i, d) in PROJ.iter().enumerate() {
        let Ok(root) = v.open_root(Path::new(d), RootRole::Other, RootAccess::Read) else {
            continue;
        };
        judge(at_dir(i as u8 + 1), read_all(v, &root, &name));
    }
    if let Ok(root) = v.open_root(Path::new(STORE), RootRole::Other, RootAccess::Read)
        && let Ok(entries) = v.list_dir(&root, Some(RelPath::literal("trash")))
    {
        for e in entries {
            let Some(dir) = e.name.as_segment() else {
                continue;
            };
            // A trash entry is this file's only when its bytes say so (the toy never reads a trash entry, so no
            // injected read error can make one unreadable).
            if let Found::Bytes(b) = read_all(v, &root, &format!("trash/{dir}/0"))
                && b == want
            {
                judge(TRASH, Found::Bytes(b));
            }
        }
    }
    if bad || found.len() > 1 {
        return Some(BAD);
    }
    found.first().copied()
}

/// What a read of a project file found.
pub enum Found {
    Absent,
    Unreadable,
    Bytes(Vec<u8>),
}

/// Reads the whole file `name` under `root` through `v`.
pub fn read_all(v: &SimVfs, root: &moirai_vfs_sim::SimRoot, name: &str) -> Found {
    let Ok(rel) = RelPath::new(name) else {
        return Found::Absent;
    };
    let f = match v.open(root, rel, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e)
            if matches!(
                e.kind,
                moirai_vfs::VfsErrorKind::NotFound | moirai_vfs::VfsErrorKind::DeletePending
            ) =>
        {
            return Found::Absent;
        }
        Err(_) => return Found::Unreadable,
    };
    let Ok(n) = v.file_size(&f) else {
        return Found::Unreadable;
    };
    let mut b = vec![0u8; n as usize];
    match v.read_exact_at(&f, 0, &mut b) {
        Ok(()) => Found::Bytes(b),
        Err(_) => Found::Unreadable,
    }
}
