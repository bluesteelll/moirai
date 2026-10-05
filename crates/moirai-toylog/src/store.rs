//! A store handle: the process's view of one store directory ([F02]), its lock client, its open files, `HEAD` reads
//! ([F04 §8]), the chain-rule scan ([F05 §5], [F16 §7]), the sealed files a checkpoint writes, and the reader's view
//! ([F16 §8]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use moirai_vfs::{
    Access, BootIdentity, Classification, ClassifyDepth, DurabilityFailure, OpenHint, RelPath,
    RelPathBuf, RootAccess, RootRole, StoreVolume, SyncKind, Vfs, VfsError, VfsErrorKind,
};

use crate::bugs::{Bug, Bugs};
use crate::codec::{hash64, hash128, u64_at};
use crate::config::Config;
use crate::format::{
    ExtentHeadRec, InitParams, Invalid, MIN_GROUP, RECHDR, TRAILER, kind, peek_header,
    validate_record,
};
use crate::head::{Choice, HEAD_LEN, Slot, choose};
use crate::lock::{ProcLocks, ToyLocks};
use crate::state::{Group, Malformed, State};
use crate::tap::{NoTap, Note, StoreFile, Tap};

/// Why an operation of the toy failed. Every variant is an exit 7 of [F19 §10.2] unless it says otherwise; a durability
/// failure never returns (the process ends through `fail_stop`, [OS/fs §4.4.5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToyError {
    /// `store_locked`: the writer byte (or a byte of a rotation) was not granted within its bound.
    StoreLocked,
    /// `outcome_pending`: the group is appended but the flush byte was not granted within its bound (P-41).
    OutcomePending,
    /// `outcome_unknown`: the group was lost twice (P-47), or `retired` was found after an append (P-28).
    OutcomeUnknown,
    /// `store_corrupt`: an invalid group or failed read below `durable_lsn`, a malformed payload, a placement defect of a
    /// valid group ([F05 §5.4]), an extent of the wrong length.
    Corrupt(String),
    /// `store_corrupt`: `log.<n>`, which a scan from the group boundary `at` needs, does not exist — the extent that holds
    /// the 8 chain bytes before `at` ([F05 §4.3]), or an extent the scan enters at an offset other than its first byte
    /// ([F05 §5.2] check 1). A reader or an appender whose remembered bound lies in an extent that a checkpoint has
    /// retired since rebuilds its view from the newest segment set once ([F16] P-56, P-59); anywhere else it is the
    /// store's corruption, exit 7.
    ExtentMissing {
        /// The extent.
        n: u32,
        /// The group boundary whose scan needed it.
        at: u64,
    },
    /// `store_corrupt` naming `HEAD`: no slot passes its checksum, after three reads ([F04 §8.1], [F16] P-61); the
    /// operator's remedy is `moirai repair`, which rebuilds both slots from the extent heads (P-85).
    NoValidSlot,
    /// `store_corrupt` naming `HEAD`: a slot passes its checksum but fails validity ([F04 §7] checks 3–5), or two valid
    /// slots disagree ([F04 §8.1]); only a defective writer produces one ([F16] P-61). Plain `moirai repair` treats it as
    /// absent and rebuilds both slots from the extent heads ([F04 §7], P-85).
    FatalSlot(&'static str),
    /// `store_corrupt` or `sealed_size`: a file the newest slot names is missing or fails its check ([F16] P-59, P-68);
    /// `moirai repair --rebuild-from-log` rebuilds a damaged base segment ([80 §2.5] rule 8).
    Damaged {
        /// The file's name in the store directory.
        file: String,
        /// What is wrong with it.
        what: &'static str,
    },
    /// `not_a_store`: the store directory has no `HEAD` (initialisation in progress, or damaged) ([F19 §10.2]).
    NotAStore,
    /// `store_io_fault`: a read the command cannot get past failed ([F16] P-92, [F15] FM-12): the lsn of the group in a
    /// writer's scan at or above `durable_lsn`, or [`HEAD_UNREADABLE`] for `HEAD`, every read of which failed.
    IoFault(u64),
    /// `disk_full` (P-90).
    DiskFull,
    /// Any other I/O error of a write, a create or a namespace call; the command aborts without an acknowledgement.
    Io(VfsError),
    /// The lock layer refused (`LOCK` missing, damaged, not writable).
    Lock(String),
    /// A refusal of the command: `commit_too_large`, `cross_volume`, `readonly_flag`, `id_space_exhausted`, a lease
    /// held by another holder (exit 4 conflict), a missing ref.
    Refused(&'static str),
    /// `maintenance_busy`, or a liveness slot not free.
    Busy,
    /// The store was retired by `restore` (P-28).
    Retired,
    /// The location is refused by the environment guard (P-94).
    Location(String),
}

impl core::fmt::Display for ToyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ToyError {}

/// The position [`ToyError::IoFault`] names for `HEAD`, every read of which failed: no lsn is that large.
pub const HEAD_UNREADABLE: u64 = u64::MAX;

impl From<Malformed> for ToyError {
    fn from(m: Malformed) -> ToyError {
        ToyError::Corrupt(format!(
            "malformed payload of kind {} at lsn {}",
            m.kind, m.lsn
        ))
    }
}

/// The name of `log.<n>` ([F02 §6.3]).
pub fn log_name(n: u32) -> RelPathBuf {
    rel(&format!("log.{n}"))
}

/// The name of `seg.base.<g>`.
pub fn seg_name(g: u32) -> RelPathBuf {
    rel(&format!("seg.base.{g}"))
}

/// The name of `hist.<h>`.
pub fn hist_name(h: u32) -> RelPathBuf {
    rel(&format!("hist.{h}"))
}

/// A relative path of the toy's own grammar; the names the toy builds are always valid.
pub fn rel(s: &str) -> RelPathBuf {
    RelPathBuf::new(s).unwrap_or_else(|e| panic!("toylog: the store name {s:?} is invalid: {e}"))
}

/// The file name of a sealed file of `family` with number `no`.
pub fn sealed_name(family: u8, no: u32) -> RelPathBuf {
    match family {
        crate::format::family::HIST => hist_name(no),
        crate::format::family::LOG => log_name(no),
        _ => seg_name(no),
    }
}

/// The toy's sealed-file header: magic, family, number, `total_len`, the 16-byte digest of the body.
pub const SEALED_MAGIC: &[u8; 8] = b"TOYSEAL1";
/// Its length.
pub const SEALED_HDR: usize = 8 + 1 + 4 + 8 + 16;

/// Encodes a sealed file: header and body.
pub fn sealed_bytes(family: u8, no: u32, body: &[u8]) -> (Vec<u8>, [u8; 16]) {
    let total = (SEALED_HDR + body.len()) as u64;
    let (lo, hi) = hash128(body);
    let mut digest = [0u8; 16];
    digest[..8].copy_from_slice(&lo.to_le_bytes());
    digest[8..].copy_from_slice(&hi.to_le_bytes());
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(SEALED_MAGIC);
    out.push(family);
    out.extend_from_slice(&no.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&digest);
    out.extend_from_slice(body);
    (out, digest)
}

/// Checks a sealed file's bytes against its header (and, if given, the digest a record names); returns its body.
pub fn sealed_body(b: &[u8], family: u8, no: u32, digest: Option<[u8; 16]>) -> Option<&[u8]> {
    if b.len() < SEALED_HDR || &b[..8] != SEALED_MAGIC || b[8] != family {
        return None;
    }
    let f_no = u32::from_le_bytes([b[9], b[10], b[11], b[12]]);
    let total = u64_at(b, 13)?;
    if f_no != no || total != b.len() as u64 {
        return None;
    }
    let body = &b[SEALED_HDR..];
    let (lo, hi) = hash128(body);
    let mut d = [0u8; 16];
    d[..8].copy_from_slice(&lo.to_le_bytes());
    d[8..].copy_from_slice(&hi.to_le_bytes());
    if d != b[21..37] || digest.is_some_and(|x| x != d) {
        return None;
    }
    Some(body)
}

/// The reader's view ([F16 §8]): the state replayed up to its bound L0, with the chain value there (P-56).
#[derive(Clone, Debug)]
pub struct View {
    /// The replayed state.
    pub state: State,
    /// The replay bound L0.
    pub l0: u64,
    /// The chain value at L0.
    pub chain: u64,
    /// The `slot_seq` of the slot last read.
    pub slot_seq: u64,
    /// The `checkpoint_lsn` of the set the view was built from.
    pub base: u64,
}

/// Where a scan stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Stop {
    /// The limit was reached.
    Limit,
    /// An invalid group at this boundary.
    Invalid(Invalid, u64),
    /// A failed read in the group at this boundary.
    ReadError(u64),
    /// The next extent does not exist (or is an interrupted preparation) at this boundary: the end of the valid log.
    NoExtent(u64),
}

/// What a scan read.
#[derive(Clone, Debug)]
pub struct Scan {
    /// The valid groups, in order.
    pub groups: Vec<Group>,
    /// The end of the last valid group read (the scan's start when none).
    pub end: u64,
    /// The chain value at `end`.
    pub chain: u64,
    /// Why it stopped.
    pub stop: Stop,
}

/// Where a streaming scan ([`Toy::scan_each`]) ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanEnd {
    /// The end of the last valid group read (the scan's start when none).
    pub end: u64,
    /// The chain value at `end`.
    pub chain: u64,
    /// Why it stopped.
    pub stop: Stop,
    /// With [`Stop::Limit`] below the limit: the end of the valid group at `end` that the limit cuts — its records pass
    /// [F05 §5.2] and its chain trailer matches ([F05 §4.6]), and it starts below the limit and ends beyond it. `None`
    /// otherwise. A bounded replay tells by it a bound that no group boundary of the valid log meets from a log that
    /// ends below the bound ([`Toy::replay_to`], [F16] P-56).
    pub over: Option<u64>,
}

/// What a reader that starts now replays up to a group boundary ([`Toy::replay_to`]): the model of the kept-view check
/// ([F16] P-56, [80 §2.4.3] "Readers").
#[derive(Clone, Debug)]
pub enum Replay {
    /// The replay reached the bound: the state of the newest slot's segment set and the valid log after it up to there.
    Reached(Box<State>),
    /// The valid log as read now has no group boundary at the bound: a valid group spans it, from `start`, below the
    /// bound, to `end`, beyond it.
    Spanned {
        /// The group's first lsn.
        start: u64,
        /// The lsn after its last byte.
        end: u64,
    },
    /// The valid log as read now ends below the bound: the scan from the newest set finds an invalid group at `end`
    /// ([F05 §5.2], [F16] P-53).
    Short {
        /// The boundary of the invalid group, the end of the valid log.
        end: u64,
    },
    /// No replay to the bound exists now, and none tells anything about it: the newest slot is of another epoch, its
    /// segment set folds beyond the bound, or the scan stops below the bound at a failed read ([F15] FM-12) or at the
    /// end of the extents (an extent that does not exist or is short, which an external truncation leaves, FM-10).
    Unreached,
}

/// The bytes of one extent a scan has read so far.
struct Window {
    /// The extent.
    n: u32,
    /// The extent offset of `bytes[0]`.
    base: u64,
    /// The bytes read, from `base` on.
    bytes: Vec<u8>,
    /// The bytes are a retired extent's `hist` copy, taken out of [`Toy::hist`] for the scan and given back after it
    /// (never copied, never slid).
    hist: bool,
}

/// A scan drops the bytes of the groups it has consumed once they reach this many, so a replay of a whole extent holds
/// a window of about this size plus one group, not the extent.
const SLIDE: u64 = 1 << 16;

/// The slot facts a scan validates against ([F05 §5.2] check 6, §9.28).
#[derive(Copy, Clone, Debug)]
pub struct ScanCtx {
    /// E.
    pub e: u64,
    /// `epoch`.
    pub epoch: u64,
    /// `epoch_lsn`.
    pub epoch_lsn: u64,
    /// `init`.
    pub init: InitParams,
    /// `project_oid_algo`.
    pub algo: u8,
}

impl ScanCtx {
    /// The context of `s`.
    pub fn of(s: &Slot) -> ScanCtx {
        ScanCtx {
            e: s.init.log_extent_bytes,
            epoch: s.epoch,
            epoch_lsn: s.epoch_lsn,
            init: s.init,
            algo: s.project_oid_algo,
        }
    }

    /// The extent of lsn `l`: n(L) = (L >> k) + 1.
    pub fn extent(&self, l: u64) -> u32 {
        (l / self.e + 1) as u32
    }

    /// The offset of lsn `l` in its extent.
    pub fn offset(&self, l: u64) -> u64 {
        l % self.e
    }

    /// The first lsn of extent `n`.
    pub fn start_of(&self, n: u32) -> u64 {
        u64::from(n - 1) * self.e
    }
}

/// One store handle of one process. Generic over the seam (`V: Vfs`, [80 §1] X4) and the protocol-note sink.
pub struct Toy<V: Vfs, T: Tap = NoTap> {
    pub(crate) vfs: V,
    pub(crate) cfg: Config,
    pub(crate) tap: T,
    pub(crate) dir: PathBuf,
    pub(crate) root: V::Root,
    pub(crate) vol: StoreVolume,
    pub(crate) locks: ToyLocks<V>,
    pub(crate) head: V::File,
    pub(crate) extents: BTreeMap<u32, V::File>,
    pub(crate) boot: BootIdentity,
    pub(crate) booted: bool,
    pub(crate) view: Option<View>,
    /// The bytes of retired extents that `repair` rebuilds from (their `hist` copies), by extent.
    pub(crate) hist: BTreeMap<u32, Vec<u8>>,
    /// The project directories of `file mv` and `file rm` (index 1…); index 0 is the store's `trash/`.
    pub(crate) project: Vec<PathBuf>,
    /// P-16's seeded bug: the rename it issues between the intent's append and its phase 2b.
    pub(crate) pending_ns: Option<crate::intent::PendingNs>,
    /// This handle has made the store directory's entries durable (see [`Toy::ensure_named`]).
    pub(crate) named: bool,
    /// P-69's and P-81's seeded bugs: ref moves and pins acknowledged with their commit or fork and written later, by
    /// this handle's next operation or when it is dropped.
    pub(crate) later: Vec<crate::ops::Op>,
    /// P-51's seeded bug runs maintenance inside a holding of the writer and flush bytes: the acquisitions it makes
    /// there are nested in that holding (writer, flush), never a second kernel acquisition by one client, which the
    /// grant table refuses ([OS/lock] contract item 3). Always zero with every switch off.
    pub(crate) nested: (u32, u32),
    /// The `WriterDiag` acquisition counter of this handle's lock client ([F03 §6.1] `seq`): 1 at its first acquisition
    /// of the writer byte, incremented at each later one.
    pub(crate) diag_seq: u64,
    /// P-63's seeded bug: the `HEAD` flush that `quiet` reported success before, issued by this handle's next
    /// operation or when it is dropped.
    pub(crate) head_flush_due: bool,
}

impl<V: Vfs, T: Tap> Drop for Toy<V, T> {
    /// P-69's, P-81's and P-63's seeded bugs write what they left for later when the handle goes away (never while
    /// unwinding).
    fn drop(&mut self) {
        if (!self.later.is_empty() || self.head_flush_due) && !std::thread::panicking() {
            self.run_later();
        }
    }
}

impl<V: Vfs, T: Tap> Toy<V, T> {
    /// Opens the store in `dir` ([OS/env §4]: the location is classified at every open; [F02 §3]). `proc` is the record
    /// the handles of this process share ([`ProcLocks`]). A directory without `HEAD` is no store, `not_a_store` ([F02
    /// §3.2], [F19 §10.2]): a missing directory, or one that holds an `init` in progress ([F16] P-88), whose `LOCK` may not
    /// exist yet either.
    pub fn open(
        vfs: V,
        dir: &Path,
        cfg: Config,
        tap: T,
        proc: ProcLocks,
    ) -> Result<Self, ToyError> {
        let root = match vfs.open_root(dir, RootRole::Store, RootAccess::ReadWrite) {
            Ok(r) => r,
            Err(e) if e.kind == VfsErrorKind::NotFound => return Err(ToyError::NotAStore),
            Err(e) => return Err(ToyError::Io(e)),
        };
        let vol = match vfs
            .classify(&root, ClassifyDepth::Open)
            .map_err(ToyError::Io)?
        {
            Classification::Local(v) => v,
            Classification::Refused(r) => return Err(ToyError::Location(format!("{r:?}"))),
        };
        let locks = match ToyLocks::open(&vfs, &root, proc, cfg.bugs) {
            Ok(l) => l,
            Err(e) => {
                let no_head = matches!(
                    vfs.open(&root, RelPath::literal("HEAD"), Access::Read, OpenHint::Normal),
                    Err(h) if h.kind == VfsErrorKind::NotFound
                );
                return Err(if no_head {
                    ToyError::NotAStore
                } else {
                    ToyError::Lock(e.to_string())
                });
            }
        };
        let head = vfs
            .open(
                &root,
                RelPath::literal("HEAD"),
                Access::ReadWrite,
                OpenHint::Normal,
            )
            .map_err(|e| {
                if e.kind == VfsErrorKind::NotFound {
                    ToyError::NotAStore
                } else {
                    ToyError::Io(e)
                }
            })?;
        // [OS/proc §4.4]: the boot identity is read once and cached for the process's life.
        let boot = vfs.boot_identity();
        Ok(Toy {
            vfs,
            cfg,
            tap,
            dir: dir.to_path_buf(),
            root,
            vol,
            locks,
            head,
            extents: BTreeMap::new(),
            boot,
            booted: false,
            view: None,
            hist: BTreeMap::new(),
            project: Vec::new(),
            pending_ns: None,
            named: false,
            later: Vec::new(),
            nested: (0, 0),
            diag_seq: 0,
            head_flush_due: false,
        })
    }

    /// The seam.
    pub fn vfs(&self) -> &V {
        &self.vfs
    }

    /// The configuration.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// The store directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The seeded bugs.
    pub(crate) fn bugs(&self) -> Bugs {
        self.cfg.bugs
    }

    /// The process's cached boot identity.
    pub fn boot(&self) -> BootIdentity {
        self.boot
    }

    /// The last view the process replayed, if any.
    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    pub(crate) fn note(&self, n: Note) {
        self.tap.note(n);
    }

    /// The note of a step that begins (`true`) or ends.
    pub(crate) fn step(&self, n: fn(bool) -> Note, begin: bool) {
        self.tap.note(n(begin));
    }

    // ---- errors ----

    /// An I/O error of a write, a create or a namespace call ([F16] P-90: `DiskFull` aborts without an
    /// acknowledgement). P-90's seeded bug treats `DiskFull` as success.
    pub(crate) fn io<R: Default>(&self, r: Result<R, VfsError>) -> Result<R, ToyError> {
        match r {
            Ok(v) => Ok(v),
            Err(e)
                if e.kind == VfsErrorKind::DiskFull
                    || e.kind == VfsErrorKind::InsufficientSpace =>
            {
                if self.bugs().on(Bug::P90DfAckAfterDiskFull) {
                    Ok(R::default())
                } else {
                    Err(ToyError::DiskFull)
                }
            }
            Err(e) => Err(ToyError::Io(e)),
        }
    }

    /// A flush of a non-lazy class ([F16] P-91): any error ends the process through `fail_stop`. P-91's seeded bug
    /// treats an error other than `Io` as success; P-44's retries the flush once on the same handle and goes on when
    /// the retry succeeds.
    pub(crate) fn durable(
        &self,
        r: Result<(), DurabilityFailure>,
        retry: impl FnOnce() -> Result<(), DurabilityFailure>,
    ) {
        let Err(f) = r else { return };
        if self.bugs().on(Bug::P91FlushErrorAsSuccess) && f.kind != VfsErrorKind::Io {
            return;
        }
        if self.bugs().on(Bug::P44FlushRetriedAndAcked) && retry().is_ok() {
            return;
        }
        self.vfs.fail_stop(f)
    }

    /// `sync` with the error policy.
    pub(crate) fn sync_file(&self, f: &V::File, kind: SyncKind) {
        self.durable(self.vfs.sync(f, kind), || self.vfs.sync(f, kind));
    }

    /// `durable-name` on `tmp/` and on the store directory once per handle, before the handle's first write can be
    /// acknowledged ([F16] P-88 "The window after step 6's rename", spec sync 2b S2B-P-34).
    ///
    /// P-88 step 6 renames `tmp/head.<nonce>` onto `HEAD` and only then runs `durable-name` on both directories; a
    /// process that discovers the store in between (an `init` that died there) could acknowledge commits whose store a
    /// crash then loses with `HEAD`'s name (the rename has two parents and is durable only once both are synced, [F15]
    /// FM-2.4). So every process, before it first acknowledges a durable effect through a store it opened (a durable
    /// group, P-46; a durable publish's success, P-13), runs `durable-name` on `tmp/` and on the store directory, once
    /// per opening, holding no role byte; measurements 1 and 2 include the two flushes.
    pub(crate) fn ensure_named(&mut self) {
        if !self.named {
            self.sync_store_dir(Some(RelPath::literal("tmp")));
            self.sync_store_dir(None);
            self.named = true;
        }
    }

    /// `sync_dir` of the store directory (or `dir` under it) with the error policy.
    pub(crate) fn sync_store_dir(&self, dir: Option<RelPath<'_>>) {
        self.durable(self.vfs.sync_dir(&self.root, dir), || {
            self.vfs.sync_dir(&self.root, dir)
        });
    }

    // ---- HEAD ----

    /// Reads both slots with one `read_at` and chooses one ([F04 §8.1], [F16] P-61, P-92): both absent, or a failed read,
    /// is read again at most twice more. When every read failed, no slot was read at all: the refusal is the read's,
    /// `store_io_fault` ([F15] FM-12.2: a persistent read error fails every read; [F16] P-92), not "no valid slot", which
    /// `repair` answers and which needs slots that read as absent.
    pub(crate) fn read_head(&self) -> Result<(Slot, usize), ToyError> {
        let mut buf = [0u8; HEAD_LEN];
        let mut failed = 0;
        for _ in 0..3 {
            match self.read_slots(&mut buf) {
                Ok(n) if n == HEAD_LEN => {}
                Ok(_) => continue,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            }
            match choose(&buf, self.bugs().on(Bug::P61FatalSlotSkipped)) {
                Choice::Newest(s, which) => return Ok((*s, which)),
                Choice::Fatal(m) => return Err(ToyError::FatalSlot(m)),
                Choice::NoneValid => {}
            }
        }
        Err(if failed == 3 {
            ToyError::IoFault(HEAD_UNREADABLE)
        } else {
            ToyError::NoValidSlot
        })
    }

    /// One read of both slots of `HEAD` into `buf`, noted as a use of the slot file ([`Note::Uses`]).
    pub(crate) fn read_slots(&self, buf: &mut [u8; HEAD_LEN]) -> Result<usize, VfsError> {
        self.note(Note::Uses(StoreFile::Head));
        self.vfs.read_at(&self.head, 0, buf)
    }

    // ---- extents ----

    /// Opens `log.<n>` into the handle cache; `false` when the file does not exist. The handle is then
    /// [`Toy::ext`]`(n)`.
    pub(crate) fn open_extent(&mut self, n: u32) -> Result<bool, ToyError> {
        Ok(self.extent(n)?.is_some())
    }

    /// [`Toy::open_extent`] for a read that may reject the extent and use nothing of it: `repair`'s look at an extent
    /// head that validates only at its position and in its epoch ([F16] P-54, P-55, P-85). It is no use of the file
    /// ([`Note::Uses`]); the scan of an extent whose head `repair` accepts uses it.
    pub(crate) fn probe_extent(&mut self, n: u32) -> Result<bool, ToyError> {
        Ok(self.extent_handle(n)?.is_some())
    }

    /// The cached handle of `log.<n>`.
    pub(crate) fn ext(&self, n: u32) -> Option<&V::File> {
        self.extents.get(&n)
    }

    /// The open handle of `log.<n>`, or `None` when the file does not exist. Every call is a use of the extent
    /// ([`Note::Uses`]): its callers read it, or need it and find it missing.
    pub(crate) fn extent(&mut self, n: u32) -> Result<Option<&V::File>, ToyError> {
        self.note(Note::Uses(StoreFile::log(n)));
        self.extent_handle(n)
    }

    fn extent_handle(&mut self, n: u32) -> Result<Option<&V::File>, ToyError> {
        if !self.extents.contains_key(&n) {
            match self.vfs.open(
                &self.root,
                log_name(n).as_rel_path(),
                Access::ReadWrite,
                OpenHint::Normal,
            ) {
                Ok(f) => {
                    self.extents.insert(n, f);
                }
                Err(e)
                    if matches!(e.kind, VfsErrorKind::NotFound | VfsErrorKind::DeletePending) =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(ToyError::Io(e)),
            }
        }
        Ok(self.extents.get(&n))
    }

    /// Forgets the handle of `log.<n>` (after the file was replaced or deleted).
    pub(crate) fn forget_extent(&mut self, n: u32) {
        self.extents.remove(&n);
    }

    /// The chain value at the group boundary `p` ([F05 §4.3]): `XXH3-64(epoch)` at `epoch_lsn`, else the 8 bytes before.
    ///
    /// `durable` is the `durable_lsn` of the slot the caller works from. A failed read of the 8 bytes is judged by their
    /// position ([F16] P-92, [F05 §5.3]): with `p` ≤ `durable` they lie below `durable_lsn`, and the read is corruption
    /// (`store_corrupt`); above it the result is `store_io_fault` naming `p`, which stops a writer's scan that starts
    /// there. A reader asks for a chain value above `durable_lsn` only to re-check its remembered bound (P-56), where
    /// any error drops the view, and then starts again from the newest set's bound, which never lies above
    /// `durable_lsn` ([F04 §7] check 5): its visible log never ends in a refusal there.
    pub(crate) fn chain_at(
        &mut self,
        ctx: &ScanCtx,
        p: u64,
        durable: u64,
    ) -> Result<u64, ToyError> {
        if p == ctx.epoch_lsn {
            return Ok(hash64(&ctx.epoch.to_le_bytes()));
        }
        let q = p - TRAILER as u64;
        let n = ctx.extent(q);
        let off = ctx.offset(q);
        if let Some(h) = self.hist.get(&n) {
            let at = off as usize;
            return u64_at(h, at)
                .ok_or_else(|| ToyError::Corrupt(format!("hist of log.{n} is short")));
        }
        if !self.open_extent(n)? {
            return Err(ToyError::ExtentMissing { n, at: p });
        }
        let Some(f) = self.ext(n) else {
            return Err(ToyError::ExtentMissing { n, at: p });
        };
        let mut b = [0u8; 8];
        if self.vfs.read_exact_at(f, off, &mut b).is_err() {
            return Err(if p <= durable {
                ToyError::Corrupt(format!(
                    "unreadable log at {q} below durable_lsn {durable}; run moirai repair"
                ))
            } else {
                ToyError::IoFault(p)
            });
        }
        Ok(u64::from_le_bytes(b))
    }

    /// Scans the log from the group boundary `from`, whose chain value is `chain`, by the chain rule ([F05 §5],
    /// [F16] P-53–P-55), up to `limit` (exclusive) or the end of the valid log, collecting the groups.
    pub(crate) fn scan(
        &mut self,
        ctx: &ScanCtx,
        from: u64,
        chain: u64,
        limit: Option<u64>,
    ) -> Result<Scan, ToyError> {
        self.scan_opt(ctx, from, chain, limit, false)
    }

    /// [`Toy::scan`], keeping every group's bytes when `keep_raw` (for a re-write).
    pub(crate) fn scan_opt(
        &mut self,
        ctx: &ScanCtx,
        from: u64,
        chain: u64,
        limit: Option<u64>,
        keep_raw: bool,
    ) -> Result<Scan, ToyError> {
        let mut groups = Vec::new();
        let end = self.scan_each(ctx, from, chain, limit, keep_raw, &mut |g| {
            groups.push(g);
            Ok(())
        })?;
        Ok(Scan {
            groups,
            end: end.end,
            chain: end.chain,
            stop: end.stop,
        })
    }

    /// The streaming form of [`Toy::scan`]: every valid group goes to `sink` as soon as it is read, so a replay holds one
    /// group and a sliding window of its extent, never the extent or the list of its groups. An error of `sink` ends the
    /// scan with that error.
    pub(crate) fn scan_each(
        &mut self,
        ctx: &ScanCtx,
        from: u64,
        chain: u64,
        limit: Option<u64>,
        keep_raw: bool,
        sink: &mut dyn FnMut(Group) -> Result<(), ToyError>,
    ) -> Result<ScanEnd, ToyError> {
        let mut win: Option<Window> = None;
        let r = self.scan_windowed(ctx, (from, chain), limit, keep_raw, sink, &mut win);
        self.give_back(win);
        r
    }

    /// Returns a `hist` window's bytes to [`Toy::hist`].
    fn give_back(&mut self, win: Option<Window>) {
        if let Some(w) = win
            && w.hist
        {
            self.hist.insert(w.n, w.bytes);
        }
    }

    fn scan_windowed(
        &mut self,
        ctx: &ScanCtx,
        (from, chain): (u64, u64),
        limit: Option<u64>,
        keep_raw: bool,
        sink: &mut dyn FnMut(Group) -> Result<(), ToyError>,
        win: &mut Option<Window>,
    ) -> Result<ScanEnd, ToyError> {
        let bugs = self.bugs();
        let e = ctx.e;
        let mut out = ScanEnd {
            end: from,
            chain,
            stop: Stop::Limit,
            over: None,
        };
        let mut p = from;
        let mut chain = chain;
        'groups: loop {
            if limit.is_some_and(|l| p >= l) {
                out.stop = Stop::Limit;
                break;
            }
            let n = ctx.extent(p);
            let off0 = ctx.offset(p);
            // The extent's existence and length ([F05 §2.2], §5.2 check 1), once per extent.
            if win.as_ref().is_none_or(|w| w.n != n) {
                let old = win.take();
                self.give_back(old);
                if let Some(h) = self.hist.remove(&n) {
                    // A retired extent's bytes from its hist copy (repair).
                    if h.len() as u64 != e {
                        self.hist.insert(n, h);
                        return Err(ToyError::Corrupt(format!(
                            "the hist copy of log.{n} is short"
                        )));
                    }
                    *win = Some(Window {
                        n,
                        base: 0,
                        bytes: h,
                        hist: true,
                    });
                } else {
                    if !self.open_extent(n)? {
                        if off0 == 0 {
                            out.stop = Stop::NoExtent(p);
                            break;
                        }
                        return Err(ToyError::ExtentMissing { n, at: p });
                    }
                    let Some(f) = self.ext(n) else {
                        return Err(ToyError::ExtentMissing { n, at: p });
                    };
                    let size = self.vfs.file_size(f).map_err(|_| ToyError::IoFault(p))?;
                    if size > e {
                        return Err(ToyError::Corrupt(format!(
                            "log.{n} is longer than the extent size ({size} > {e}); run moirai doctor --fsck"
                        )));
                    }
                    if size < e {
                        if off0 == 0 {
                            // An interrupted preparation beyond the valid log: never read as log (P-72).
                            out.stop = Stop::NoExtent(p);
                            break;
                        }
                        return Err(ToyError::Corrupt(format!(
                            "log.{n} is shorter than the extent size ({size} < {e}); run moirai doctor --fsck"
                        )));
                    }
                    *win = Some(Window {
                        n,
                        base: off0,
                        bytes: Vec::new(),
                        hist: false,
                    });
                }
            }
            // Slide the window past the groups already consumed.
            if let Some(w) = win.as_mut()
                && !w.hist
                && off0 >= w.base + SLIDE
            {
                let k = ((off0 - w.base) as usize).min(w.bytes.len());
                w.bytes.drain(..k);
                w.base += k as u64;
            }
            let gstart = p;
            let mut q = p;
            let mut recs = Vec::new();
            loop {
                let off = ctx.offset(q);
                if off + RECHDR as u64 > e || ctx.extent(q) != n {
                    out.stop = Stop::Invalid(Invalid::Unterminated, gstart);
                    break 'groups;
                }
                // The header first, then the whole record.
                if self.fill(win, n, off, RECHDR as u64, e).is_err() {
                    out.stop = Stop::ReadError(gstart);
                    break 'groups;
                }
                let len = win.as_ref().map_or(0, |w| {
                    let rel = (off - w.base) as usize;
                    peek_header(&w.bytes[rel..]).map_or(0, |h| u64::from(h.0))
                });
                let want = len.clamp(RECHDR as u64, e.saturating_sub(off).max(RECHDR as u64));
                if self.fill(win, n, off, want, e).is_err() {
                    out.stop = Stop::ReadError(gstart);
                    break 'groups;
                }
                let Some(w) = win.as_ref() else {
                    out.stop = Stop::ReadError(gstart);
                    break 'groups;
                };
                match validate_in(w, off, e, q, ctx.epoch, bugs) {
                    Ok((r, end, len)) => {
                        recs.push(r);
                        q += len;
                        if end {
                            break;
                        }
                    }
                    Err(inv) => {
                        out.stop = Stop::Invalid(inv, gstart);
                        break 'groups;
                    }
                }
            }
            // A group that ends beyond the limit is not read (the limit is a publish's bound). Its chain trailer is
            // checked all the same, so that the scan says whether a valid group spans the limit ([`ScanEnd::over`]).
            if limit.is_some_and(|l| q > l) {
                out.stop = Stop::Limit;
                out.over = win
                    .as_ref()
                    .and_then(|w| chain_trailer(w, ctx, (gstart, q), chain, bugs))
                    .map(|_| q);
                break;
            }
            // The chain trailer ([F05 §4.6]).
            let Some(w) = win.as_ref() else {
                out.stop = Stop::ReadError(gstart);
                break;
            };
            let Some(trailer) = chain_trailer(w, ctx, (gstart, q), chain, bugs) else {
                out.stop = Stop::Invalid(Invalid::Chain, gstart);
                break;
            };
            let raw = if keep_raw {
                let a = (ctx.offset(gstart) - w.base) as usize;
                let z = (ctx.offset(q - 1) + 1 - w.base) as usize;
                w.bytes[a..z].to_vec()
            } else {
                Vec::new()
            };
            // [F05 §5.4] (spec sync 2b S2B-P-22): two placement defects of a valid group are corrupt wherever they lie,
            // above durable_lsn included, since only a defective writer produces a checksummed, chained group like that:
            // a group that leaves 1–39 bytes in its extent (G-4 broken, §4.4), and a first group of an extent that is
            // not one `ExtentHead` record (§4.5, [F16] P-97).
            let left = (e - ctx.offset(q)) % e;
            if (1..MIN_GROUP).contains(&left) {
                return Err(ToyError::Corrupt(format!(
                    "the valid group at {gstart} leaves {left} bytes in log.{n} (G-4); run moirai doctor --fsck"
                )));
            }
            if off0 == 0 && (recs.len() != 1 || recs[0].kind != kind::EXTENT_HEAD) {
                return Err(ToyError::Corrupt(format!(
                    "the first group of log.{n} is not its extent head; run moirai doctor --fsck"
                )));
            }
            // An extent head is alone in its group, at an extent's first byte, with the chain value there and the
            // slot's epoch_lsn, init and algorithm ([F05 §9.28]); otherwise it is malformed (corrupt wherever it lies).
            for r in &recs {
                if r.kind == kind::EXTENT_HEAD {
                    let h = ExtentHeadRec::decode(&r.body).map_err(|_| {
                        ToyError::Corrupt(format!("malformed extent head at {}", r.lsn))
                    })?;
                    if recs.len() != 1
                        || ctx.offset(r.lsn) != 0
                        || h.chain_in != chain
                        || h.epoch_lsn != ctx.epoch_lsn
                        || h.init != ctx.init
                        || h.project_oid_algo != ctx.algo
                    {
                        return Err(ToyError::Corrupt(format!(
                            "malformed extent head at {}",
                            r.lsn
                        )));
                    }
                }
            }
            sink(Group {
                start: gstart,
                end: q,
                chain_in: chain,
                chain_out: trailer,
                recs,
                raw,
            })?;
            chain = trailer;
            p = q;
            out.end = p;
            out.chain = chain;
        }
        Ok(out)
    }

    /// Makes the window hold the bytes `[off, off + len)` of extent `n`, reading forward from its end.
    fn fill(&self, win: &mut Option<Window>, n: u32, off: u64, len: u64, e: u64) -> Result<(), ()> {
        let need_end = (off + len).min(e);
        let w = win.get_or_insert_with(|| Window {
            n,
            base: off,
            bytes: Vec::new(),
            hist: false,
        });
        if w.n != n || off < w.base {
            *w = Window {
                n,
                base: off,
                bytes: Vec::new(),
                hist: false,
            };
        }
        let have_end = w.base + w.bytes.len() as u64;
        if have_end >= need_end {
            return Ok(());
        }
        // Read ahead, bounded by the extent: at least 4 KiB, doubling with the window up to 64 KiB, so a scan of a short
        // pending range (the common case at every append and flush) reads a few KiB, and a replay of a whole extent
        // reads it in few calls.
        let ahead = (w.bytes.len() as u64).clamp(1 << 12, 1 << 16);
        let chunk = (need_end - have_end).max(ahead).min(e - have_end);
        let start = w.bytes.len();
        let Some(f) = self.extents.get(&n) else {
            return Err(());
        };
        w.bytes.resize(start + chunk as usize, 0);
        let mut got = 0usize;
        while got < chunk as usize {
            match self
                .vfs
                .read_at(f, have_end + got as u64, &mut w.bytes[start + got..])
            {
                Ok(0) => break,
                Ok(k) => got += k,
                Err(_) => {
                    w.bytes.truncate(start);
                    return Err(());
                }
            }
        }
        w.bytes.truncate(start + got);
        if w.base + w.bytes.len() as u64 >= need_end {
            Ok(())
        } else {
            Err(())
        }
    }

    // ---- sealed files ----

    /// Reads a whole sealed file, or `None` when it is missing or unreadable.
    pub(crate) fn read_file(&self, name: RelPath<'_>) -> Option<Vec<u8>> {
        let f = self
            .vfs
            .open(&self.root, name, Access::Read, OpenHint::Sequential)
            .ok()?;
        let size = self.vfs.file_size(&f).ok()?;
        let mut b = vec![0u8; size as usize];
        self.vfs.read_exact_at(&f, 0, &mut b).ok()?;
        Some(b)
    }

    // ---- the reader's view ----

    /// The newest valid slot, after the boot check that precedes a process's first read ([F16] P-60): a process whose
    /// boot identity is Known and differs from the slot's `boot_id` runs boot-change recovery first (P-66). P-60's seeded
    /// bug skips the check.
    ///
    /// The handle counts as booted only once the check has passed: the slot carries the process's boot identity (no
    /// recovery needed, or boot-change recovery ended successfully, which leaves it in both slots), or the process is
    /// in Unknown-boot mode (P-67). A recovery that refuses (`store_locked` while a dead process's flush byte is not yet
    /// released, `outcome_pending`, `store_io_fault`) leaves the handle unbooted, so its next read runs the check again
    /// instead of replaying a slot of the earlier boot.
    pub fn head_for_read(&mut self) -> Result<Slot, ToyError> {
        let (s, _) = self.read_head()?;
        if self.booted {
            return Ok(s);
        }
        if let BootIdentity::Known(b) = self.boot
            && b.0 != s.boot_id
            && !self.bugs().on(Bug::P60T8NoBootCheck)
        {
            self.boot_recover()?;
            self.booted = true;
            return Ok(self.read_head()?.0);
        }
        self.booted = true;
        Ok(s)
    }

    /// A fresh state for this handle's configuration (facts kept when [`Config::facts`]).
    pub(crate) fn fresh_state(&self) -> State {
        State::new().keeping_facts(self.cfg.facts)
    }

    /// Loads the segment set of `s` into a fresh state ([F16] P-59): a named file that is missing or damaged is re-read
    /// with the newest slot once, and then refuses (exit 7); a reader never falls back to an older set.
    pub(crate) fn load_set(&mut self, s: &Slot) -> Result<(State, u64), ToyError> {
        match self.try_load(s) {
            Ok(v) => Ok(v),
            Err(first) => {
                let (again, _) = self.read_head()?;
                if again.segments != s.segments {
                    return self.try_load(&again);
                }
                Err(first)
            }
        }
    }

    /// The slot that is not the newest, if valid (the unit tests' check of a durable publish).
    #[cfg(test)]
    pub(crate) fn other_slot(&self) -> Option<Slot> {
        let mut buf = [0u8; HEAD_LEN];
        let ok = matches!(self.read_slots(&mut buf), Ok(n) if n == HEAD_LEN);
        if !ok {
            return None;
        }
        let a = Slot::read(&buf[..crate::head::SLOT_LEN]);
        let b = Slot::read(&buf[crate::head::SLOT_LEN..]);
        match (a, b) {
            (crate::head::SlotRead::Valid(x), crate::head::SlotRead::Valid(y)) => {
                Some(if x.slot_seq < y.slot_seq { *x } else { *y })
            }
            _ => None,
        }
    }

    fn try_load(&mut self, s: &Slot) -> Result<(State, u64), ToyError> {
        let Some(seg) = s.segments.first() else {
            return Ok((self.fresh_state(), s.epoch_lsn));
        };
        let name = seg_name(seg.file_no);
        self.note(Note::Uses(StoreFile::Sealed {
            family: crate::format::family::SEG_BASE,
            no: seg.file_no,
        }));
        let damaged = |what| ToyError::Damaged {
            file: name.as_str().to_owned(),
            what,
        };
        let b = self
            .read_file(name.as_rel_path())
            .ok_or_else(|| damaged("it is missing or cannot be read"))?;
        let body = sealed_body(
            &b,
            crate::format::family::SEG_BASE,
            seg.file_no,
            Some(seg.digest),
        )
        .ok_or_else(|| damaged("it does not match its record (sealed_size)"))?;
        let (st, upto) = State::from_snapshot(body, self.cfg.facts)
            .map_err(|_| damaged("its snapshot is malformed"))?;
        if upto != seg.upto_lsn {
            return Err(damaged("it folds another bound than its record"));
        }
        Ok((st, upto))
    }

    /// Brings the view up to the newest slot's `committed_lsn` and returns that slot ([F16] P-56–P-59): readers take no
    /// lock byte and never read past `committed_lsn`; an invalid group or a failed read at or above `durable_lsn` ends the
    /// visible log, below it is corruption. Groups are applied as the scan reads them; a scan that ends in an error
    /// drops the view (the next refresh rebuilds it from the newest set).
    pub fn refresh(&mut self) -> Result<Slot, ToyError> {
        let s = self.head_for_read()?;
        let ctx = ScanCtx::of(&s);
        let bugs = self.bugs();
        // P-56: a remembered bound is re-checked whenever the slot changed.
        if let Some(v) = &self.view
            && v.slot_seq != s.slot_seq
            && !bugs.on(Bug::P56OverlayKeptAfterRefill)
        {
            let (l0, chain) = (v.l0, v.chain);
            let keep = s.committed_lsn >= l0 && l0 >= s.checkpoint_lsn.min(v.base) && {
                let now = self.chain_at(&ctx, l0, s.durable_lsn);
                now.is_ok_and(|c| c == chain)
            };
            if !keep {
                self.view = None;
            }
        }
        let mut view = match self.view.take() {
            Some(v) => v,
            None => {
                let (state, upto) = self.load_set(&s)?;
                let chain = self.chain_at(&ctx, upto, s.durable_lsn)?;
                View {
                    state,
                    l0: upto,
                    chain,
                    slot_seq: s.slot_seq,
                    base: upto,
                }
            }
        };
        // P-57: readers replay only up to committed_lsn; its seeded bug replays to the end of the valid log.
        let limit = if bugs.on(Bug::P57T3ReaderPastCommitted) {
            None
        } else {
            Some(s.committed_lsn)
        };
        let r = self.scan_each(&ctx, view.l0, view.chain, limit, false, &mut |g| {
            view.state.apply(&g, bugs, false).map_err(ToyError::from)
        });
        let end = match r {
            Ok(end) => end,
            Err(ToyError::ExtentMissing { .. }) => {
                // An extent the view needed was retired: rebuild from the newest set (P-59), once; a missing extent there
                // is the store's corruption.
                return self.refresh_from_set(&s, limit);
            }
            Err(e) => return Err(e),
        };
        self.judge_reader_stop(&s, &end.stop)?;
        view.slot_seq = s.slot_seq;
        view.l0 = end.end;
        view.chain = end.chain;
        self.view = Some(view);
        Ok(s)
    }

    fn refresh_from_set(&mut self, s: &Slot, limit: Option<u64>) -> Result<Slot, ToyError> {
        let ctx = ScanCtx::of(s);
        let bugs = self.bugs();
        let (mut state, upto) = self.load_set(s)?;
        let chain = self.chain_at(&ctx, upto, s.durable_lsn)?;
        let end = self.scan_each(&ctx, upto, chain, limit, false, &mut |g| {
            state.apply(&g, bugs, false).map_err(ToyError::from)
        })?;
        self.judge_reader_stop(s, &end.stop)?;
        self.view = Some(View {
            state,
            l0: end.end,
            chain: end.chain,
            slot_seq: s.slot_seq,
            base: upto,
        });
        Ok(s.clone())
    }

    /// What a reader that starts now replays up to the group boundary `bound` of epoch `epoch`, leaving this process's
    /// view alone: the newest slot's segment set and the valid log after it up to `bound`, pending groups included (no
    /// `committed_lsn` limit: the bound is a view's, which a scan from an earlier slot may have reached). The test
    /// harness's kept-view check ([F16 §17.2] "model", [F16] P-56) compares a view a process kept with it: after the
    /// re-check, a view equals the replay of the valid log from the selected slot's segment set up to the view's bound
    /// ([80 §2.4.3] "Readers").
    ///
    /// [`Replay::Reached`] with that state; [`Replay::Spanned`] when the valid log as read now has no group boundary at
    /// `bound` because a valid group spans it (the scan reads the group that the bound cuts, its chain trailer included,
    /// [`ScanEnd::over`]); [`Replay::Short`] when the valid log ends below `bound` with an invalid group;
    /// [`Replay::Unreached`] when no such replay exists now and nothing is known about the bound: the newest slot is of
    /// another epoch, its set folds beyond `bound`, or the scan stops below `bound` at a failed read or at the end of the
    /// extents.
    pub fn replay_to(&mut self, bound: u64, epoch: u64) -> Result<Replay, ToyError> {
        let s = self.head_for_read()?;
        if s.epoch != epoch {
            return Ok(Replay::Unreached);
        }
        let ctx = ScanCtx::of(&s);
        let bugs = self.bugs();
        let (mut state, upto) = self.load_set(&s)?;
        if upto > bound {
            return Ok(Replay::Unreached);
        }
        if upto == bound {
            return Ok(Replay::Reached(Box::new(state)));
        }
        let chain = self.chain_at(&ctx, upto, s.durable_lsn)?;
        let end = self.scan_each(&ctx, upto, chain, Some(bound), false, &mut |g| {
            state.apply(&g, bugs, false).map_err(ToyError::from)
        })?;
        Ok(match (end.stop, end.over) {
            _ if end.end == bound => Replay::Reached(Box::new(state)),
            (Stop::Limit, Some(over)) => Replay::Spanned {
                start: end.end,
                end: over,
            },
            // Below the bound, a limit stop cut a group whose records are valid and whose trailer is not: that group
            // is invalid ([F16] P-53), like the one an invalid stop names.
            (Stop::Limit, None) | (Stop::Invalid(..), None) => Replay::Short { end: end.end },
            _ => Replay::Unreached,
        })
    }

    /// [F16] P-58, P-92 for readers: a stop below `durable_lsn` is corruption; P-58's seeded bug ends the view there.
    fn judge_reader_stop(&self, s: &Slot, stop: &Stop) -> Result<(), ToyError> {
        let p = match stop {
            Stop::Invalid(_, p) | Stop::ReadError(p) | Stop::NoExtent(p) => *p,
            Stop::Limit => return Ok(()),
        };
        if p < s.durable_lsn && !self.bugs().on(Bug::P58CorruptionAsEndOfView) {
            return Err(ToyError::Corrupt(format!(
                "the log is invalid at {p}, below durable_lsn {}; run moirai repair",
                s.durable_lsn
            )));
        }
        Ok(())
    }

    /// `doctor --verify`'s `HEAD` check (I-G6, [F16] P-50): the published slot names the segment set of the newest
    /// covered `Checkpoint` that the view replayed. `None` when they agree.
    pub fn head_fold_problem(&mut self) -> Option<String> {
        let s = self.refresh().ok()?;
        let v = self.view.as_ref()?;
        (s.segments != v.state.segments).then(|| {
            format!(
                "I-G6 (P-50): HEAD names the segment set {:?}; the newest covered Checkpoint set is {:?}",
                s.segments.iter().map(|x| x.file_no).collect::<Vec<_>>(),
                v.state.segments.iter().map(|x| x.file_no).collect::<Vec<_>>()
            )
        })
    }

    /// The state as a reader sees it now: refresh, then the view's state.
    pub fn read(&mut self) -> Result<&State, ToyError> {
        self.refresh()?;
        self.view
            .as_ref()
            .map(|v| &v.state)
            .ok_or_else(|| ToyError::Corrupt("no view".to_owned()))
    }
}

/// Validates the record at extent offset `off` of window `w` ([F05 §5.2] checks 2–7). `validate_record` works on a buffer
/// whose offset 0 is the extent's offset 0: it gets the window's bytes with the offsets shifted by the window's base, and
/// the extent's remaining length as its bound.
fn validate_in(
    w: &Window,
    off: u64,
    e: u64,
    p: u64,
    epoch: u64,
    bugs: Bugs,
) -> Result<(crate::format::RecView, bool, u64), Invalid> {
    let rel = (off - w.base) as usize;
    let bound = w.base + w.bytes.len() as u64;
    let room = e.min(bound) - w.base;
    validate_record(&w.bytes[..room as usize], rel, e - w.base, p, epoch, bugs)
}

/// The chain trailer of the group `[start, end)`, whose records window `w` holds, when it equals the hash of the group's
/// bytes before it seeded with `chain` ([F05 §4.6]), else `None`. P-53's seeded bug accepts any trailer.
fn chain_trailer(
    w: &Window,
    ctx: &ScanCtx,
    (start, end): (u64, u64),
    chain: u64,
    bugs: Bugs,
) -> Option<u64> {
    let a = (ctx.offset(start) - w.base) as usize;
    let z = (ctx.offset(end - 1) + 1 - w.base) as usize;
    let trailer = u64_at(&w.bytes, z - TRAILER).unwrap_or(0);
    let computed = crate::codec::hash64_seeded(&w.bytes[a..z - TRAILER], chain);
    (computed == trailer || bugs.on(Bug::P53G9NoChainCheck)).then_some(trailer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Counters, HEAD_GROUP, MIN_GROUP, Rec, encode_group, group_len};
    use crate::testing::{EPOCH, STORE, open, sim_store};
    use moirai_vfs::StoreFs;
    use moirai_vfs_sim::{SimVfs, SimWorld};
    use proptest::prelude::*;

    /// E of the scan tests: twice the slide threshold, so a replay of one extent slides its window.
    const E: u64 = 1 << 17;

    fn cfg() -> Config {
        let mut c = Config::test_profile();
        c.extent_bytes = E;
        c
    }

    /// One group as laid out: its start, end and trailer.
    type Laid = (u64, u64, u64);

    fn put(exts: &mut Vec<Vec<u8>>, lsn: u64, b: &[u8]) {
        let n = (lsn / E) as usize;
        while exts.len() <= n {
            exts.push(vec![0u8; E as usize]);
        }
        let off = (lsn % E) as usize;
        exts[n][off..off + b.len()].copy_from_slice(b);
    }

    /// Lays out groups of `Noop` records with the payload sizes of `sizes` after the image's groups, by G-3: a group that
    /// does not fit the rest of its extent is preceded by the pad and the next extent's head. Returns the extents' bytes
    /// (each E long) and every group from the image's first, in order.
    fn layout(image: &[u8], sizes: &[Vec<usize>]) -> (Vec<Vec<u8>>, Vec<Laid>) {
        let init = cfg().init_params([7; 16]);
        let mut exts = vec![vec![0u8; E as usize]];
        exts[0][..image.len()].copy_from_slice(image);
        // The image's two groups: the extent head at 0 and the group that creates main.
        let t0 = u64_at(image, HEAD_GROUP as usize - TRAILER).unwrap_or(0);
        let t1 = u64_at(image, image.len() - TRAILER).unwrap_or(0);
        let mut laid = vec![(0, HEAD_GROUP, t0), (HEAD_GROUP, image.len() as u64, t1)];
        let mut at = image.len() as u64;
        let mut chain = t1;
        for g in sizes {
            let recs: Vec<Rec> = g
                .iter()
                .map(|&n| Rec::new(kind::NOOP, vec![0; n], Bugs::NONE))
                .collect();
            let len = group_len(&recs);
            let r = E - at % E;
            let at_start = at.is_multiple_of(E);
            if at_start || !(len == r || len + MIN_GROUP <= r) {
                if !at_start {
                    let pad = Rec::new(kind::NOOP, vec![0; (r - MIN_GROUP) as usize], Bugs::NONE);
                    let mut b = Vec::new();
                    chain = encode_group(&[pad], at, EPOCH, chain, &mut b);
                    put(&mut exts, at, &b);
                    laid.push((at, at + r, chain));
                    at += r;
                }
                let h = ExtentHeadRec {
                    epoch_lsn: 0,
                    chain_in: chain,
                    init,
                    project_oid_algo: 1,
                    hflags: 0,
                    counters: Counters::EMPTY,
                };
                let mut b = Vec::new();
                chain = encode_group(
                    &[Rec::new(kind::EXTENT_HEAD, h.encode(), Bugs::NONE)],
                    at,
                    EPOCH,
                    chain,
                    &mut b,
                );
                put(&mut exts, at, &b);
                laid.push((at, at + b.len() as u64, chain));
                at += b.len() as u64;
            }
            let mut b = Vec::new();
            chain = encode_group(&recs, at, EPOCH, chain, &mut b);
            put(&mut exts, at, &b);
            laid.push((at, at + len, chain));
            at += len;
        }
        (exts, laid)
    }

    /// Writes the extents' bytes: log.1 (the image's) in place, the others as new files.
    fn place(w: &SimWorld, exts: &[Vec<u8>]) {
        for (k, b) in exts.iter().enumerate().skip(1) {
            w.put_file(&Path::new(STORE).join(format!("log.{}", k + 1)), b)
                .unwrap_or_else(|e| panic!("log.{}: {e:?}", k + 1));
        }
        let v = w.process_with("layout", None, Some(true));
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap_or_else(|e| panic!("{e}"));
        let f = v
            .open(
                &root,
                RelPath::literal("log.1"),
                Access::ReadWrite,
                OpenHint::Normal,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        v.write_at(&f, 0, &exts[0])
            .unwrap_or_else(|e| panic!("{e}"));
    }

    fn ctx_of(t: &Toy<SimVfs>) -> ScanCtx {
        ScanCtx::of(&t.read_head().unwrap_or_else(|e| panic!("{e}")).0)
    }

    fn unlink(v: &SimVfs, name: &str) {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap_or_else(|e| panic!("{e}"));
        v.unlink(
            &root,
            RelPath::new(name).unwrap_or_else(|e| panic!("{e}")),
            moirai_vfs::ShareRetry::None,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    fn laid_of(s: &Scan) -> Vec<Laid> {
        s.groups
            .iter()
            .map(|g| (g.start, g.end, g.chain_out))
            .collect()
    }

    fn groups(sizes: impl Strategy<Value = usize>) -> impl Strategy<Value = Vec<Vec<usize>>> {
        proptest::collection::vec(proptest::collection::vec(sizes, 1..4), 20..70)
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        /// Chained groups across extent boundaries, scanned from any group boundary (up to any later boundary, or to the
        /// end), come back in their encoded order with their trailers, and the scan stops at the end of the valid log.
        #[test]
        fn a_scan_from_any_boundary_returns_the_encoded_groups(
            sizes in groups(0usize..3_000),
            from in any::<prop::sample::Index>(),
            to in any::<prop::sample::Index>(),
            limited in any::<bool>(),
        ) {
            let c = cfg();
            let (w, v, img) = sim_store(&c, 11);
            let (exts, laid) = layout(&img.log, &sizes);
            place(&w, &exts);
            let mut t = open(&v, &c);
            let ctx = ctx_of(&t);
            let i = from.index(laid.len());
            let start = laid[i].0;
            let chain = t.chain_at(&ctx, start, u64::MAX).unwrap();
            let j = i + to.index(laid.len() - i);
            let limit = limited.then(|| laid[j].0);
            let sc = t.scan(&ctx, start, chain, limit).unwrap();
            let want: Vec<Laid> = if limited { laid[i..j].to_vec() } else { laid[i..].to_vec() };
            prop_assert_eq!(&laid_of(&sc), &want);
            // Each group's chain_in is its predecessor's trailer.
            let prev = std::iter::once(chain).chain(want.iter().map(|x| x.2));
            for (g, p) in sc.groups.iter().zip(prev) {
                prop_assert_eq!(g.chain_in, p);
            }
            let end = want.last().map_or(start, |x| x.1);
            prop_assert_eq!(sc.end, end);
            if limited {
                prop_assert_eq!(sc.stop, Stop::Limit);
            } else if end.is_multiple_of(E) {
                prop_assert_eq!(sc.stop, Stop::NoExtent(end));
            } else {
                prop_assert!(matches!(sc.stop, Stop::Invalid(_, p) if p == end), "{:?}", sc.stop);
            }
        }

        /// A torn or zeroed tail inside group k (from any byte up to its trailer, to the end of its extent) ends the
        /// valid log at group k's start with an invalid group, whatever boundary before it the scan starts from.
        #[test]
        fn a_zeroed_tail_stops_the_scan_at_the_damaged_group(
            sizes in groups(0usize..3_000),
            from in any::<prop::sample::Index>(),
            damaged in any::<prop::sample::Index>(),
            cut in any::<prop::sample::Index>(),
        ) {
            let c = cfg();
            let (w, v, img) = sim_store(&c, 12);
            let (mut exts, laid) = layout(&img.log, &sizes);
            // The image's own groups stay intact (the chain seed at 0 comes from HEAD's epoch).
            let k = 2 + damaged.index(laid.len() - 2);
            let (ks, ke, _) = laid[k];
            let p = ks + cut.index((ke - TRAILER as u64 - ks + 1) as usize) as u64;
            let n = (p / E) as usize;
            for b in &mut exts[n][(p % E) as usize..] {
                *b = 0;
            }
            // The later extents are gone too: the tail of the log is what a crash left.
            exts.truncate(n + 1);
            place(&w, &exts);
            let mut t = open(&v, &c);
            let ctx = ctx_of(&t);
            let i = from.index(k + 1);
            let start = laid[i].0;
            let chain = t.chain_at(&ctx, start, u64::MAX).unwrap();
            let sc = t.scan(&ctx, start, chain, None).unwrap();
            prop_assert_eq!(&laid_of(&sc), &laid[i..k].to_vec());
            prop_assert_eq!(sc.end, ks);
            prop_assert!(matches!(sc.stop, Stop::Invalid(_, q) if q == ks), "{:?}", sc.stop);
        }

        /// [`Toy::replay_to`] (the kept-view model, [F16] P-56) reaches exactly the group boundaries of the valid log,
        /// pending groups included (the slot's `committed_lsn` is the image's end): a replay at every group's end; at any
        /// byte inside a group, that group spanning the bound; beyond the valid log, its end (the invalid group there, or
        /// the end of the extents); nothing in another epoch.
        #[test]
        fn a_replay_reaches_exactly_the_group_boundaries(
            sizes in groups(0usize..3_000),
            at in any::<prop::sample::Index>(),
            inside in any::<prop::sample::Index>(),
        ) {
            let c = cfg();
            let (w, v, img) = sim_store(&c, 13);
            let (exts, laid) = layout(&img.log, &sizes);
            place(&w, &exts);
            let mut t = open(&v, &c);
            let (gs, ge, _) = laid[at.index(laid.len())];
            let last = laid.last().map_or(0, |x| x.1);
            let got = t.replay_to(ge, EPOCH).unwrap();
            prop_assert!(matches!(got, Replay::Reached(_)), "{:?}", got);
            let got = t.replay_to(ge, EPOCH + 1).unwrap();
            prop_assert!(matches!(got, Replay::Unreached), "{:?}", got);
            // Every group is at least MIN_GROUP long, so the bound lies strictly inside it.
            let mid = gs + 1 + inside.index((ge - gs - 1) as usize) as u64;
            let got = t.replay_to(mid, EPOCH).unwrap();
            prop_assert!(
                matches!(got, Replay::Spanned { start, end } if start == gs && end == ge),
                "{:?}",
                got
            );
            let got = t.replay_to(last + 1, EPOCH).unwrap();
            if last.is_multiple_of(E) {
                prop_assert!(matches!(got, Replay::Unreached), "{:?}", got);
            } else {
                prop_assert!(matches!(got, Replay::Short { end } if end == last), "{:?}", got);
            }
        }
    }

    /// A limit inside a group ([`ScanEnd::over`], [`Replay::Spanned`]): the group counts as spanning the limit only when
    /// it is valid, its chain trailer included ([F05 §4.6]); with a damaged trailer the valid log ends below the limit,
    /// at that group ([`Replay::Short`]).
    #[test]
    fn a_group_spans_a_limit_only_when_its_trailer_is_valid() {
        let c = cfg();
        let (w, v, img) = sim_store(&c, 14);
        let (mut exts, laid) = layout(&img.log, &[vec![100], vec![200, 300], vec![50]]);
        let (gs, ge, _) = laid[3];
        let mid = gs + (ge - gs) / 2;
        place(&w, &exts);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let chain = t.chain_at(&ctx, gs, u64::MAX).unwrap();
        let end = t
            .scan_each(&ctx, gs, chain, Some(mid), false, &mut |_| Ok(()))
            .unwrap();
        assert_eq!(
            (end.end, end.stop.clone(), end.over),
            (gs, Stop::Limit, Some(ge))
        );
        let got = t.replay_to(mid, EPOCH).unwrap();
        assert!(
            matches!(got, Replay::Spanned { start, end } if (start, end) == (gs, ge)),
            "{got:?}"
        );
        // At a group boundary the limit cuts nothing.
        let end = t
            .scan_each(&ctx, gs, chain, Some(ge), false, &mut |_| Ok(()))
            .unwrap();
        assert_eq!((end.end, end.over), (ge, None));
        // One bit of the group's trailer flipped.
        exts[0][(ge - 1) as usize] ^= 1;
        place(&w, &exts);
        let end = t
            .scan_each(&ctx, gs, chain, Some(mid), false, &mut |_| Ok(()))
            .unwrap();
        assert_eq!((end.end, end.over), (gs, None));
        let got = t.replay_to(mid, EPOCH).unwrap();
        assert!(matches!(got, Replay::Short { end } if end == gs), "{got:?}");
        let got = t.replay_to(gs, EPOCH).unwrap();
        assert!(matches!(got, Replay::Reached(_)), "{got:?}");
    }

    /// [`Toy::replay_to`] at a reader's view bound holds the view's commits and runtime rows; a set that folds beyond the
    /// bound gives no replay, and at the set's own bound the replay is the set's state.
    #[test]
    fn a_replay_to_a_view_bound_equals_the_view() {
        let c = Config::test_profile();
        let (_w, v, _) = sim_store(&c, 21);
        let mut t = open(&v, &c);
        let rt = crate::ops::Op::Runtime(crate::ops::RuntimeOp {
            op: 7,
            rows: vec![(7, 70)],
            pad: 0,
            symbols: Vec::new(),
            target_len: 0,
        });
        for o in [commit(1), rt, commit(2)] {
            t.run(&o).unwrap_or_else(|e| panic!("{e}"));
        }
        let mut r = open(&v, &c);
        r.refresh().unwrap_or_else(|e| panic!("{e}"));
        let view = r.view().cloned().unwrap_or_else(|| panic!("a view"));
        assert!(view.state.commits.contains_key(&2) && view.state.runtime.get(&7) == Some(&70));
        let Replay::Reached(got) = r
            .replay_to(view.l0, EPOCH)
            .unwrap_or_else(|e| panic!("{e}"))
        else {
            panic!("a replay to the view's bound");
        };
        assert!(got.commits.keys().eq(view.state.commits.keys()));
        assert_eq!(got.runtime, view.state.runtime);
        let ck = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the checkpoint ran"));
        let below = r.replay_to(ck.upto - 1, EPOCH);
        assert!(matches!(below, Ok(Replay::Unreached)), "{below:?}");
        let Replay::Reached(at) = r
            .replay_to(ck.upto, EPOCH)
            .unwrap_or_else(|e| panic!("{e}"))
        else {
            panic!("the set's state");
        };
        assert!(at.commits.contains_key(&1) && at.commits.contains_key(&2));
    }

    /// [F04 §8.1] with [F15] FM-12: a transient read error of `HEAD` is read again; a persistent one fails every read,
    /// and the refusal is the read's (`store_io_fault`), not "no valid slot", which `repair` answers.
    #[test]
    fn an_unreadable_head_is_an_io_fault() {
        let c = cfg();
        let (w, v, _) = sim_store(&c, 12);
        let t = open(&v, &c);
        let head = w
            .node_at(&Path::new(STORE).join("HEAD"))
            .unwrap_or_else(|| panic!("HEAD"));
        w.queue_choice_on(None, moirai_vfs_sim::Site::ReadFault, head, 1);
        assert!(t.read_head().is_ok());
        w.queue_choice_on(None, moirai_vfs_sim::Site::ReadFault, head, 2);
        assert_eq!(
            t.read_head().map(|_| ()),
            Err(ToyError::IoFault(HEAD_UNREADABLE))
        );
    }

    /// [F16] P-92, [F05 §5.3]: a failed read of the chain bytes before a group boundary is corruption when they lie below
    /// the slot's `durable_lsn`, and `store_io_fault` naming the boundary above it; a missing extent is typed.
    #[test]
    fn a_failed_chain_read_is_judged_by_its_position() {
        let c = cfg();
        let (w, v, _) = sim_store(&c, 19);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let log1 = w
            .node_at(&Path::new(STORE).join("log.1"))
            .unwrap_or_else(|| panic!("log.1"));
        let p = HEAD_GROUP;
        w.queue_choice_on(None, moirai_vfs_sim::Site::ReadFault, log1, 1);
        assert!(matches!(
            t.chain_at(&ctx, p, p),
            Err(ToyError::Corrupt(m)) if m.contains("below durable_lsn")
        ));
        w.queue_choice_on(None, moirai_vfs_sim::Site::ReadFault, log1, 1);
        assert_eq!(t.chain_at(&ctx, p, 0), Err(ToyError::IoFault(p)));
        assert!(t.chain_at(&ctx, p, p).is_ok(), "the fault was transient");
        assert_eq!(
            t.chain_at(&ctx, c.extent_bytes + p, u64::MAX),
            Err(ToyError::ExtentMissing {
                n: 2,
                at: c.extent_bytes + p
            })
        );
    }

    /// A tap that keeps the store files of [`Note::Uses`].
    #[derive(Clone, Default)]
    struct UsesTap(std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<StoreFile>>>);

    impl Tap for UsesTap {
        fn note(&self, n: Note) {
            if let Note::Uses(f) = n {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(f);
            }
        }
    }

    impl UsesTap {
        fn take(&self) -> Vec<StoreFile> {
            let mut g = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *g).into_iter().collect()
        }
    }

    /// [`Note::Uses`] ([F16 §17.2] avail: a `store_corrupt` refusal concerns the files the state it refuses from rests
    /// on): a refresh that builds its view names the slot file, the segment file it loads and the extent it scans; a
    /// scan that needs an extent and finds it missing names it too.
    #[test]
    fn an_operation_notes_the_store_files_it_uses() {
        let c = Config::test_profile();
        let (_w, v, _) = sim_store(&c, 23);
        let mut t = open(&v, &c);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        let ck = t
            .checkpoint()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("the checkpoint ran"));
        t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        let tap = UsesTap::default();
        let mut r = Toy::open(
            v.clone(),
            Path::new(STORE),
            c.clone(),
            tap.clone(),
            ProcLocks::new(),
        )
        .unwrap_or_else(|e| panic!("open: {e}"));
        tap.take();
        r.refresh().unwrap_or_else(|e| panic!("{e}"));
        let seg = StoreFile::Sealed {
            family: crate::format::family::SEG_BASE,
            no: ck.segment,
        };
        assert_eq!(tap.take(), vec![StoreFile::Head, StoreFile::log(1), seg]);
        assert_eq!(seg.name().as_str(), format!("seg.base.{}", ck.segment));
        assert_eq!(StoreFile::Head.name().as_str(), "HEAD");
        let ctx = ScanCtx::of(&r.read_head().unwrap_or_else(|e| panic!("{e}")).0);
        let p = c.extent_bytes + HEAD_GROUP;
        assert_eq!(
            r.chain_at(&ctx, p, u64::MAX),
            Err(ToyError::ExtentMissing { n: 2, at: p })
        );
        assert_eq!(tap.take(), vec![StoreFile::Head, StoreFile::log(2)]);
        assert_eq!(StoreFile::log(2).name().as_str(), "log.2");
    }

    #[test]
    fn a_retired_extent_is_scanned_from_its_hist_copy_and_given_back() {
        let c = cfg();
        let (w, v, img) = sim_store(&c, 13);
        let sizes: Vec<Vec<usize>> = (0..60).map(|k| vec![1_000 + 70 * k]).collect();
        let (exts, laid) = layout(&img.log, &sizes);
        assert!(exts.len() >= 2, "the layout spans extents");
        place(&w, &exts);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let seed = hash64(&EPOCH.to_le_bytes());
        let direct = t.scan(&ctx, 0, seed, None).unwrap();
        assert_eq!(laid_of(&direct), laid);
        // log.1 retired: its bytes come from the hist copy, the file is gone.
        t.forget_extent(1);
        unlink(&v, "log.1");
        t.hist.insert(1, exts[0].clone());
        let from_hist = t.scan(&ctx, 0, seed, None).unwrap();
        assert_eq!(laid_of(&from_hist), laid);
        assert_eq!(
            t.hist.get(&1).map(Vec::len),
            Some(E as usize),
            "the hist bytes are given back"
        );
        // A short hist copy is corruption.
        t.hist.insert(1, vec![0; 10]);
        assert!(matches!(
            t.scan(&ctx, 0, seed, None),
            Err(ToyError::Corrupt(_))
        ));
    }

    #[test]
    fn extent_length_rules() {
        let c = cfg();
        let sizes: Vec<Vec<usize>> = (0..70).map(|_| vec![2_000]).collect();
        let seed = hash64(&EPOCH.to_le_bytes());
        // An extent shorter than E beyond the valid log is an interrupted preparation: the end of the log.
        let (w, v, img) = sim_store(&c, 14);
        let (exts, laid) = layout(&img.log, &sizes);
        let last = exts.len() - 1;
        let first_of_last = laid
            .iter()
            .position(|x| x.0 == last as u64 * E)
            .unwrap_or(laid.len());
        let mut short = exts.clone();
        short[last].truncate(100);
        place(&w, &short);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let sc = t.scan(&ctx, 0, seed, None).unwrap();
        assert_eq!(sc.stop, Stop::NoExtent(last as u64 * E));
        assert_eq!(sc.groups.len(), first_of_last);
        // An extent longer than E is corrupt wherever it lies.
        let (w, v, _) = sim_store(&c, 15);
        let mut long = exts.clone();
        long[0].extend_from_slice(&[0; 16]);
        place(&w, &long);
        let mut t = open(&v, &c);
        assert!(
            matches!(t.scan(&ctx, 0, seed, None), Err(ToyError::Corrupt(m)) if m.contains("longer"))
        );
        // An extent missing inside the log (not at its first byte) is corrupt.
        let (w, v, _) = sim_store(&c, 16);
        place(&w, &exts);
        let mut t = open(&v, &c);
        unlink(&v, "log.2");
        let (mid, _, _) = laid
            .iter()
            .copied()
            .find(|x| x.0 > E && x.0 % E != 0)
            .unwrap_or((E + 200, 0, 0));
        let chain = u64_at(&exts[1], (mid % E) as usize - TRAILER).unwrap_or(0);
        assert_eq!(
            t.scan(&ctx, mid, chain, None).map(|s| s.end),
            Err(ToyError::ExtentMissing { n: 2, at: mid })
        );
    }

    /// A commit on `main` with its idempotency key.
    fn commit(op: u64) -> crate::ops::Op {
        crate::ops::Op::Commit(crate::ops::CommitOp {
            op,
            digest: op,
            ref_name: crate::state::MAIN,
            creates: vec![op << 8],
            key: Some(op),
            filler: 200,
            ..crate::ops::CommitOp::default()
        })
    }

    /// [F05 §5.4] (spec sync 2b S2B-P-22): a valid, chained group that leaves 1–39 bytes in its extent (G-4) is corrupt
    /// wherever it lies. Every scan that reads it stops with exit 7, a writer's scan above `durable_lsn` included, so no
    /// append overwrites it; a reader, which reads only the visible log, never reaches it there.
    #[test]
    fn a_valid_group_that_breaks_g4_is_corrupt_above_durable_lsn_too() {
        let c = cfg();
        let seed = hash64(&EPOCH.to_le_bytes());
        let (w, v, img) = sim_store(&c, 17);
        let (mut exts, laid) = layout(&img.log, &[]);
        let (at, chain) = (laid[1].1, laid[1].2);
        let len = E - at - 20;
        let rec = Rec::new(kind::NOOP, vec![0; (len - MIN_GROUP) as usize], Bugs::NONE);
        let mut b = Vec::new();
        encode_group(&[rec], at, EPOCH, chain, &mut b);
        put(&mut exts, at, &b);
        place(&w, &exts);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let r = t.scan(&ctx, 0, seed, None);
        assert!(
            matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("leaves 20 bytes")),
            "{r:?}"
        );
        // A scan limited to the boundary before it never reads it.
        assert!(t.scan(&ctx, 0, seed, Some(at)).is_ok());
        // The group lies above the published durable_lsn: a reader's view ends before it, a writer refuses the store.
        assert!(t.refresh().is_ok());
        let r = t.run(&commit(1));
        assert!(
            matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("G-4")),
            "{r:?}"
        );
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.committed_lsn, at, "nothing was appended or published");
    }

    /// [F05 §5.4], §4.5 (spec sync 2b S2B-P-22; [F16] P-97): an extent whose first valid group is not one `ExtentHead`
    /// record is corrupt wherever it lies.
    #[test]
    fn an_extent_whose_first_group_is_not_its_head_is_corrupt() {
        let c = cfg();
        let seed = hash64(&EPOCH.to_le_bytes());
        let (w, v, img) = sim_store(&c, 18);
        let (mut exts, laid) = layout(&img.log, &[]);
        let (at, chain) = (laid[1].1, laid[1].2);
        // The pad to the end of log.1, then a chained `Noop` group at log.2's first byte in place of its extent head.
        let pad = Rec::new(
            kind::NOOP,
            vec![0; (E - at - MIN_GROUP) as usize],
            Bugs::NONE,
        );
        let mut b = Vec::new();
        let chain = encode_group(&[pad], at, EPOCH, chain, &mut b);
        put(&mut exts, at, &b);
        let mut b = Vec::new();
        encode_group(
            &[Rec::new(kind::NOOP, vec![0; 100], Bugs::NONE)],
            E,
            EPOCH,
            chain,
            &mut b,
        );
        put(&mut exts, E, &b);
        place(&w, &exts);
        let mut t = open(&v, &c);
        let ctx = ctx_of(&t);
        let r = t.scan(&ctx, 0, seed, None);
        assert!(
            matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("the first group of log.2 is not its extent head")),
            "{r:?}"
        );
        // The pad before it is valid: a scan limited to log.2's first byte ends there.
        let sc = t
            .scan(&ctx, 0, seed, Some(E))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(sc.end, E);
    }

    #[test]
    fn sealed_files_check_their_header_and_digest() {
        let fam = crate::format::family::SEG_BASE;
        let (b, d) = sealed_bytes(fam, 5, b"body");
        assert_eq!(sealed_body(&b, fam, 5, Some(d)), Some(&b"body"[..]));
        assert_eq!(sealed_body(&b, fam, 6, None), None);
        assert_eq!(sealed_body(&b, crate::format::family::HIST, 5, None), None);
        assert_eq!(sealed_body(&b[..b.len() - 1], fam, 5, None), None);
        assert_eq!(sealed_body(&b, fam, 5, Some([0; 16])), None);
        let mut flipped = b.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert_eq!(sealed_body(&flipped, fam, 5, None), None);
    }
}
