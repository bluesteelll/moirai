//! `SimVfs`: the in-memory `Vfs` of one simulated process — `StoreFs`, `Locks`, `SealedMaps`, `EnvGuard`, `Clock`,
//! `ProcHost` and `Entropy` over the simulated kernel of [`crate::SimWorld`].
//!
//! Every call starts at a scheduling point and ends with a `Return` event on every path; reads, writes, flushes and
//! `sync_dir` have a second point between their start and their return, so other tasks can act inside them (FM-4, FM-6,
//! FM-11). Every fault site of [F15 §3] is a choice of the adversary at the point the rule names.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use moirai_vfs::{
    Access, Acquired, Advice, BootIdentity, Classification, ClassifyDepth, Clock, DirEntry,
    DurabilityClass, DurabilityFailure, Entropy, EntryKind, EntryName, EnvGuard, EnvWarning,
    ExtentMethod, FileIdentity, FreeSpace, Grant, GroupMember, LockByte, LockError, LockMode,
    Locks, MapError, OpenHint, OsCode, OsTag, OsVersion, ParentRec, ProbeOutcome, ProbeReport,
    ProbeResult, ProcHost, ProcId, Refusal, RelPath, RootAccess, RootRole, SealedMap, SealedMaps,
    ShareRetry, StoreFs, StoreVolume, SwapOutcome, SwapRecovery, SyncKind, UnknownBoot,
    VfsCounters, VfsError, VfsErrorKind, VfsTypes, Wake, WatchEvent,
};

use crate::adversary::{PartialWrite, Site};
use crate::content::ZEROS;
use crate::locks::{self, SimClient};
use crate::namespace::{Kind, NsOp};
use crate::trace::{EventKind, le_padded};
use crate::world::{
    Block, CallKind, Ctx, DeathCause, DeathPlan, FlushWhat, InFlightFlush, InFlightRead,
    InFlightWrite, Shared, SpawnRequest, State, ViolationKind, WriteBytes, error_code, os_code,
    unwind,
};

/// The simulated `Vfs` of one simulated process. Cheap to clone; clones are the same process (several clients, several
/// tasks).
#[derive(Clone)]
pub struct SimVfs {
    pub(crate) sh: Arc<Shared>,
    pub(crate) proc: u32,
}

impl core::fmt::Debug for SimVfs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimVfs").field("proc", &self.proc).finish()
    }
}

/// An open directory of the simulated namespace. It follows the directory's identity, not its path.
#[derive(Clone, Debug)]
pub struct SimRoot {
    pub(crate) node: u64,
    pub(crate) role: RootRole,
    pub(crate) access: RootAccess,
}

impl SimRoot {
    /// The role it was opened with.
    pub fn role(&self) -> RootRole {
        self.role
    }

    /// The directory's node.
    pub fn node(&self) -> u64 {
        self.node
    }
}

/// An open file of one simulated process. Dropping it closes the handle (completing a delete-pending unlink when it is
/// the last one, FM-8.3).
pub struct SimFile {
    pub(crate) sh: Arc<Shared>,
    pub(crate) handle: u64,
    pub(crate) node: u64,
    pub(crate) proc: u32,
    pub(crate) access: Access,
}

impl SimFile {
    /// The file's node (the `a` value of the trace's file events and the key of the crash surface).
    pub fn node(&self) -> u64 {
        self.node
    }
}

impl core::fmt::Debug for SimFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimFile")
            .field("node", &self.node)
            .field("proc", &self.proc)
            .field("access", &self.access)
            .finish()
    }
}

impl Drop for SimFile {
    fn drop(&mut self) {
        let mut g = self.sh.lock();
        g.close_handle(self.handle);
    }
}

/// One content a mapping has shown, kept for as long as the mapping lives (the borrow [`SealedMap::bytes`] returns).
struct View {
    bytes: Arc<[u8]>,
    next: OnceLock<Box<View>>,
}

/// A mapping of a sealed file (FM-9). Every access through [`SealedMap::bytes`] is a mapped read of the file's current
/// content, coherent with the cache as a real mapping is (an external rewrite shows, FM-10.1): the file's snapshot, one
/// shared buffer for every mapping until its content changes, with poisoned sub-sectors drawn afresh per access
/// (FM-3.2). Each access checks for an external truncation below the mapped length — the process dies, or the bytes
/// beyond the new end read as zeros (FM-9.1, FM-9.2) — and for a media fault, injected on the file
/// ([`crate::SimWorld::inject_map_fault`]) or drawn by the adversary once per mapping at `map_sealed`
/// ([`crate::Site::MapFault`]), which ends the process at that read. The contents shown are kept in a chain that grows
/// only when the content changes, so earlier borrows stay valid.
pub struct SimMap {
    sh: Arc<Shared>,
    handle: u64,
    node: u64,
    proc: u32,
    len: u64,
    fault: bool,
    first: View,
}

impl Drop for SimMap {
    fn drop(&mut self) {
        let mut g = self.sh.lock();
        g.close_handle(self.handle);
    }
}

impl SimMap {
    /// Returns `want`'s first `len` bytes, appending it to the chain unless it equals the last content shown.
    fn keep(&self, want: Arc<[u8]>) -> &[u8] {
        let len = self.len as usize;
        let mut at = &self.first;
        while let Some(n) = at.next.get() {
            at = n;
        }
        if Arc::ptr_eq(&at.bytes, &want) || at.bytes[..len] == want[..len] {
            return &at.bytes[..len];
        }
        let n = at.next.get_or_init(|| {
            Box::new(View {
                bytes: want,
                next: OnceLock::new(),
            })
        });
        &n.bytes[..len]
    }
}

/// What a mapped read of the first `len` bytes of `node` returns now (the file is at least `len` bytes long, or the
/// caller chose the zeros of a truncated end): the shared snapshot, or a fresh buffer where a sector is poisoned or the
/// file is shorter.
fn mapped_content(st: &mut State, node: u64, proc: u32, len: u64) -> Arc<[u8]> {
    let State { ch, k, .. } = st;
    let file =
        k.ns.nodes
            .get_mut(&node)
            .and_then(|n| n.file_mut())
            .expect("simulator: a mapped file exists while it is mapped");
    if file.content.cs() < len || file.content.poisoned_below(len) {
        let mut v = vec![0u8; len as usize];
        file.content.read(0, &mut v, &mut |site, aux, n| {
            ch.pick(site, proc, node, aux, n)
        });
        return v.into();
    }
    if file.snap.is_none() {
        file.snap = Some(file.content.to_vec().into());
    }
    Arc::clone(file.snap.as_ref().expect("the snapshot was just built"))
}

impl SealedMap for SimMap {
    fn bytes(&self) -> &[u8] {
        let (len, node, proc) = (self.len, self.node, self.proc);
        let mut g = self.sh.lock();
        if !g.alive(proc) {
            drop(g);
            unwind(crate::SimUnwind::Died);
        }
        let injected =
            g.k.ns
                .nodes
                .get_mut(&node)
                .and_then(|n| n.file_mut())
                .is_some_and(|f| core::mem::take(&mut f.map_fault));
        if injected || self.fault {
            g.ev(EventKind::MapRead, None, proc, node, len, 2);
            g.kill_proc(proc, DeathCause::MapFault, &DeathPlan::default());
            drop(g);
            unwind(crate::SimUnwind::Died);
        }
        let cs = g.k.ns.file(node).content.cs();
        if cs < len {
            let zeros = g.pick(Site::MapTruncated, proc, node, cs, 2) == 1;
            g.ev(
                EventKind::MapRead,
                None,
                proc,
                node,
                len,
                if zeros { 1 } else { 3 },
            );
            if !zeros {
                g.kill_proc(proc, DeathCause::MapFault, &DeathPlan::default());
                drop(g);
                unwind(crate::SimUnwind::Died);
            }
        }
        let want = mapped_content(&mut g, node, proc, len);
        drop(g);
        self.keep(want)
    }

    fn len(&self) -> u64 {
        self.len
    }
}

/// The watch of the simulated parent ([OS/proc §7]).
#[derive(Debug)]
pub struct SimParentWatch {
    parent: Option<u32>,
}

/// A wake object of `wait_parent_or_wake`.
pub struct SimWake {
    sh: Arc<Shared>,
    id: u64,
}

impl Wake for SimWake {
    fn signal(&self) {
        let mut g = self.sh.lock();
        if let Some(set) = g.k.wakes.get_mut(&self.id) {
            *set = true;
        }
        g.wake(Block::Parent(self.id));
    }
}

impl Drop for SimWake {
    /// The wake object goes with its value, so a long run keeps only the live ones.
    fn drop(&mut self) {
        let mut g = self.sh.lock();
        g.k.wakes.remove(&self.id);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers

/// What an OS names the call behind a simulated operation ([OS/fs] Appendix A); only for errors and `fail_stop` lines.
#[derive(Copy, Clone)]
pub(crate) enum Op {
    Open,
    Mkdir,
    Rmdir,
    List,
    Read,
    Write,
    SyncData,
    SyncMeta,
    SyncDir,
    Unlink,
    Rename,
    Seal,
    SetLen,
    Stat,
    Free,
}

fn call(os: OsTag, op: Op) -> &'static str {
    let win = matches!(os, OsTag::Windows | OsTag::Unspecified);
    let mac = os == OsTag::MacOs;
    match op {
        Op::Open => {
            if win {
                "NtCreateFile"
            } else {
                "openat"
            }
        }
        Op::Mkdir => {
            if win {
                "CreateDirectoryW"
            } else {
                "mkdirat"
            }
        }
        Op::Rmdir => {
            if win {
                "RemoveDirectoryW"
            } else {
                "unlinkat"
            }
        }
        Op::List => {
            if win {
                "NtQueryDirectoryFile"
            } else {
                "getdents"
            }
        }
        Op::Read => {
            if win {
                "ReadFile"
            } else {
                "pread"
            }
        }
        Op::Write => {
            if win {
                "WriteFile"
            } else {
                "pwrite"
            }
        }
        Op::SyncData => {
            if win {
                "NtFlushBuffersFileEx"
            } else if mac {
                "fcntl(F_FULLFSYNC)"
            } else {
                "fdatasync"
            }
        }
        Op::SyncMeta => {
            if win {
                "FlushFileBuffers"
            } else if mac {
                "fcntl(F_FULLFSYNC)"
            } else {
                "fsync"
            }
        }
        Op::SyncDir => {
            if win {
                "FlushFileBuffers"
            } else {
                "fsync"
            }
        }
        Op::Unlink => {
            if win {
                "DeleteFileW"
            } else {
                "unlinkat"
            }
        }
        Op::Rename => {
            if win {
                "MoveFileExW"
            } else if mac {
                "renameatx_np"
            } else {
                "renameat2"
            }
        }
        Op::Seal => {
            if win {
                "SetFileInformationByHandle"
            } else {
                "fchmod"
            }
        }
        Op::SetLen => {
            if win {
                "SetFileInformationByHandle"
            } else {
                "ftruncate"
            }
        }
        Op::Stat => {
            if win {
                "GetFileInformationByHandleEx"
            } else {
                "fstat"
            }
        }
        Op::Free => {
            if win {
                "GetDiskFreeSpaceExW"
            } else if mac {
                "fstatfs"
            } else {
                "fstatvfs"
            }
        }
    }
}

fn windows(st: &State) -> bool {
    matches!(st.cfg.os, OsTag::Windows | OsTag::Unspecified)
}

pub(crate) fn err(st: &State, kind: VfsErrorKind, op: Op) -> VfsError {
    let os = st.cfg.os;
    VfsError::new(kind, os_code(os, kind), call(os, op))
}

/// The error of an open, by path, of a directory where a file is expected ([OS/fs §6.2]): on Windows `NtCreateFile`
/// with `FILE_NON_DIRECTORY_FILE` fails with `STATUS_FILE_IS_A_DIRECTORY`, which converts to error 5 (`AccessDenied`);
/// on Linux and macOS `EISDIR` (21), an unlisted code (`Other`).
fn open_dir_err(st: &State) -> VfsError {
    if windows(st) {
        err(st, VfsErrorKind::AccessDenied, Op::Open)
    } else {
        VfsError::new(VfsErrorKind::Other, OsCode(21), call(st.cfg.os, Op::Open))
    }
}

/// The error kind of a flush fault choice (FM-3.1: `Io`, `DiskFull` or `Unsupported`).
fn flush_fault_kind(v: u64) -> VfsErrorKind {
    match v {
        1 => VfsErrorKind::Io,
        2 => VfsErrorKind::DiskFull,
        _ => VfsErrorKind::Unsupported,
    }
}

/// A failure of a non-lazy class ([OS/fs §4.4.5]): the process must now `fail_stop`, so its later write, flush, create
/// or namespace calls are violations ([F15 §3.13]) — unless the class ran embedded in a call that reports it as a
/// `VfsError` (`create_root`, `swap_dirs`, `swap_recover`, the `init` probe), whose caller exits through its error path.
fn fail_durability(
    st: &mut State,
    proc: u32,
    embedded: bool,
    class: DurabilityClass,
    kind: VfsErrorKind,
    op: Op,
) -> DurabilityFailure {
    if !embedded {
        st.k.procs[proc as usize].failed_nonlazy = true;
    }
    let os = st.cfg.os;
    DurabilityFailure {
        class,
        call: call(os, op),
        kind,
        os: os_code(os, kind),
    }
}

/// FM-8.2: a sharing violation on this attempt, from a running streak or a new one the adversary starts. The error is
/// Windows error 32 (`SharingViolation`) or 5 (`AccessDenied`), as the adversary picks per failing attempt.
pub(crate) fn sharing_check(st: &mut State, proc: u32, node: u64) -> Result<(), VfsErrorKind> {
    let n = st.k.ns.node_mut(node);
    let fails = if n.share_block > 0 {
        if n.share_block != u64::MAX {
            n.share_block -= 1;
        }
        true
    } else {
        let v = st.pick(Site::Sharing, proc, node, 0, u64::MAX);
        if v > 0 {
            let n = st.k.ns.node_mut(node);
            n.share_block = if v == u64::MAX { u64::MAX } else { v - 1 };
        }
        v > 0
    };
    if !fails {
        return Ok(());
    }
    Err(match st.pick(Site::SharingKind, proc, node, 0, 2) {
        0 => VfsErrorKind::SharingViolation,
        _ => VfsErrorKind::AccessDenied,
    })
}

fn ns_fault(st: &mut State, proc: u32, node: u64) -> Result<(), VfsErrorKind> {
    match st.pick(Site::NsFault, proc, node, 0, 3) {
        0 => Ok(()),
        1 => Err(VfsErrorKind::DiskFull),
        _ => Err(VfsErrorKind::AccessDenied),
    }
}

/// One attempt of a namespace operation on `nodes`: the sharing check of each node, then the operation's own fault.
/// Under `ShareRetry::Bounded` on Windows, `SharingViolation` and `AccessDenied` are retried by [OS/fs §6.3]'s schedule
/// on the monotonic clock; on Linux and macOS `retry` is ignored ([OS/fs §6.3]).
pub(crate) fn attempt(
    ctx: &mut Ctx<'_>,
    nodes: &[u64],
    retry: ShareRetry,
    op: Op,
) -> Result<(), VfsError> {
    let proc = ctx.proc;
    let start = ctx.st().k.clock.mono_ns;
    let mut retries = 0u32;
    loop {
        let st = ctx.st();
        let mut r = Ok(());
        for &n in nodes {
            r = sharing_check(st, proc, n);
            if r.is_err() {
                break;
            }
        }
        if r.is_ok() {
            r = ns_fault(st, proc, nodes[0]);
        }
        let kind = match r {
            Ok(()) => return Ok(()),
            Err(k) => k,
        };
        let retryable = matches!(
            kind,
            VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
        ) && windows(st);
        if !retryable {
            return Err(err(st, kind, op));
        }
        let elapsed_ms = (st.k.clock.mono_ns - start) / 1_000_000;
        match retry.next_sleep_ms(retries, elapsed_ms) {
            None => return Err(err(st, kind, op)),
            Some(ms) => {
                st.k.procs[proc as usize].counters.share_retries += 1;
                let until = st.k.clock.boot_ns + u64::from(ms) * 1_000_000;
                ctx.block(Block::Timer, Some(until));
                retries += 1;
            }
        }
    }
}

pub(crate) fn note_nonlazy_call(st: &mut State, proc: u32, node: u64) {
    if st.k.procs[proc as usize].failed_nonlazy {
        st.violation(ViolationKind::CallAfterDurabilityFailure, proc, node);
    }
}

/// [OS/fs §6.3]: a call issued with `ShareRetry::Bounded` by a process that holds the writer or flush byte is a
/// violation, flagged once, at the call's start, whether or not a retry happens.
pub(crate) fn flag_bounded_retry(st: &mut State, proc: u32, retry: ShareRetry, node: u64) {
    if matches!(retry, ShareRetry::Bounded { .. }) && locks::holds_writer_or_flush(st, proc) {
        st.violation(ViolationKind::RetryUnderWriterOrFlush, proc, node);
    }
}

/// `ReadOnlyVolume` for an operation that changes the volume of `node`.
pub(crate) fn writable_volume(st: &State, node: u64, op: Op) -> Result<(), VfsError> {
    let vol = st.k.ns.node(node).vol;
    if st.k.volumes[vol as usize].read_only {
        return Err(err(st, VfsErrorKind::ReadOnlyVolume, op));
    }
    Ok(())
}

fn identity_of(st: &State, node: u64) -> FileIdentity {
    let vol = st.k.ns.node(node).vol;
    let mut file = [0u8; 16];
    file[..8].copy_from_slice(&node.to_le_bytes());
    FileIdentity {
        volume: 0x5349_4D00_0000_0000 | u64::from(vol),
        file,
    }
}

/// The `Return` event of a call that returns `VfsError`.
fn ret<T>(ctx: &mut Ctx<'_>, call: CallKind, node: u64, r: &Result<T, VfsError>) {
    let code = r.as_ref().map_or_else(|e| error_code(e.kind), |_| 0);
    ctx.ret_code(call, node, code);
}

impl Ctx<'_> {
    /// The `sync_dir` of directory node `dir`, with its interval ([F15 §4.1] `durable-name`, FM-2.3, FM-3.7).
    fn sync_dir_node(&mut self, dir: u64, embedded: bool) -> Result<(), DurabilityFailure> {
        let proc = self.proc;
        let st = self.st();
        note_nonlazy_call(st, proc, dir);
        let c = &mut st.k.procs[proc as usize].counters;
        c.sync_dir += 1;
        if st.cfg.os == OsTag::MacOs {
            // `fsync(dirfd)`, then the device barrier `F_FULLFSYNC` ([OS/fs §4.4.3], §4.13).
            st.k.procs[proc as usize].counters.full_barriers += 1;
        }
        let limit = st.k.ns.next_op;
        let id = st.k.next_io;
        st.k.next_io += 1;
        st.k.flushes.push(InFlightFlush {
            id,
            proc,
            node: dir,
            what: FlushWhat::Dir { limit },
        });
        st.ev(EventKind::FlushStart, None, proc, dir, 2, id);
        self.point(CallKind::SyncDir, dir, 1);
        let st = self.st();
        st.k.flushes.retain(|f| f.id != id);
        let fault = st.pick(Site::SyncDirFault, proc, dir, 0, 4);
        if fault == 0 {
            for op in st.k.ns.sync_dir_ok(dir, limit) {
                st.ev(EventKind::NsDurable, None, proc, op, 0, 0);
            }
            st.ev(EventKind::FlushEnd, None, proc, dir, 2, 0);
            Ok(())
        } else {
            st.ev(EventKind::FlushEnd, None, proc, dir, 2, 1);
            Err(fail_durability(
                st,
                proc,
                embedded,
                DurabilityClass::DurableName,
                flush_fault_kind(fault),
                Op::SyncDir,
            ))
        }
    }
}

impl SimVfs {
    pub(crate) fn enter(&self, call: CallKind, node: u64) -> Ctx<'_> {
        Ctx::enter(&self.sh, self.proc, call, node)
    }

    fn file_ctx(&self, f: &SimFile, call: CallKind) -> Ctx<'_> {
        assert_eq!(
            f.proc, self.proc,
            "simulator: a file handle used through the Vfs of another process"
        );
        let mut ctx = self.enter(call, f.node);
        if !ctx.st().k.handles.contains_key(&f.handle) {
            ctx.die();
        }
        ctx
    }

    /// The simulated process index (as the trace names it).
    pub fn process(&self) -> u32 {
        self.proc
    }

    /// Appends a harness note to the trace ([F13 §1.4]: acknowledgements, publishes, markers), attributed to this
    /// process and the calling task. Not a scheduling point.
    pub fn note(&self, tag: u64, b: u64, c: u64) {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let me = ctx.me;
        let proc = self.proc;
        ctx.st().ev(EventKind::Note, me, proc, tag, b, c);
    }

    /// Changes the size of a file (an `ftruncate` or `SetFileInformationByHandle(EndOfFile)`): the sparse extent method
    /// ([OS/fs §4.5]) and the size step of `create_extent`. A size change may fail with `DiskFull` or `Io` (FM-5.1);
    /// a failed growth may have applied in part (FM-5.2, any prefix of the zeros).
    pub fn set_len(&self, file: &SimFile, len: u64) -> Result<(), VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::SetLen);
        let proc = self.proc;
        let node = file.node;
        let st = ctx.st();
        let r = if file.access != Access::ReadWrite {
            Err(err(st, VfsErrorKind::AccessDenied, Op::SetLen))
        } else {
            note_nonlazy_call(st, proc, node);
            let fault = st.pick(Site::WriteFault, proc, node, len, 3);
            let cs = st.k.ns.file(node).content.cs();
            // A failed growth applied any prefix of its zeros (FM-5.2); a failed truncation changed nothing.
            let upto = if fault == 0 || len <= cs {
                len
            } else {
                match PartialWrite::from_choice(st.pick(
                    Site::PartialWrite,
                    proc,
                    node,
                    len,
                    u64::MAX,
                )) {
                    PartialWrite::Nothing => cs,
                    PartialWrite::Prefix(n) => cs + n.min(len - cs),
                    _ => len,
                }
            };
            if fault == 0 || len > cs {
                let State { ch, k, .. } = &mut *st;
                k.ns.edit(node, |c| {
                    c.set_len(upto, &mut |site, aux, n| ch.pick(site, proc, node, aux, n));
                });
            }
            match fault {
                0 => Ok(()),
                1 => Err(err(st, VfsErrorKind::DiskFull, Op::SetLen)),
                _ => Err(err(st, VfsErrorKind::Io, Op::SetLen)),
            }
        };
        ret(&mut ctx, CallKind::SetLen, node, &r);
        r
    }

    fn zero_fill(&self, file: &SimFile, len: u64) -> Result<(), VfsError> {
        let chunk = ZEROS.len() as u64;
        let mut off = 0;
        while off < len {
            let n = (len - off).min(chunk);
            self.write_at(file, off, &ZEROS[..n as usize])?;
            off += n;
        }
        Ok(())
    }

    /// A rename (§5.4, §5.5) as a call of its own; `flag` checks the retry rule of [OS/fs §6.3] at its start (the
    /// steps of `swap_dirs` and `swap_recover` do not: their call did).
    #[allow(clippy::too_many_arguments)] // the `StoreFs` rename arguments, plus the form and the check.
    pub(crate) fn rename_op(
        &self,
        from_root: &SimRoot,
        from: RelPath<'_>,
        to_root: &SimRoot,
        to: RelPath<'_>,
        retry: ShareRetry,
        replace: bool,
        flag: bool,
    ) -> Result<(), VfsError> {
        let kind = if replace {
            CallKind::RenameReplace
        } else {
            CallKind::RenameNoreplace
        };
        let mut ctx = self.enter(kind, from_root.node);
        let r = rename_in(&mut ctx, from_root, from, to_root, to, retry, replace, flag);
        ret(&mut ctx, kind, from_root.node, &r);
        r
    }

    /// `unlink` as a call of its own; `flag` as for [`SimVfs::rename_op`].
    pub(crate) fn unlink_op(
        &self,
        root: &SimRoot,
        rel: RelPath<'_>,
        retry: ShareRetry,
        flag: bool,
    ) -> Result<(), VfsError> {
        let mut ctx = self.enter(CallKind::Unlink, root.node);
        let r = unlink_in(&mut ctx, root, rel, retry, flag);
        ret(&mut ctx, CallKind::Unlink, root.node, &r);
        r
    }

    /// `sync(file, kind)`; `embedded` as for [`fail_durability`].
    pub(crate) fn sync_op(
        &self,
        file: &SimFile,
        kind: SyncKind,
        embedded: bool,
    ) -> Result<(), DurabilityFailure> {
        assert_eq!(
            file.access,
            Access::ReadWrite,
            "simulator: sync on a handle without write access (a programming error, [OS/fs §4.4.2])"
        );
        let mut ctx = self.file_ctx(file, CallKind::Sync);
        let proc = self.proc;
        let node = file.node;
        let meta = kind == SyncKind::DataAndMeta;
        let st = ctx.st();
        note_nonlazy_call(st, proc, node);
        if meta {
            st.k.procs[proc as usize].counters.sync_meta += 1;
        } else {
            st.k.procs[proc as usize].counters.sync_data += 1;
        }
        let mark = st.k.ns.file(node).content.flush_mark();
        let id = st.k.next_io;
        st.k.next_io += 1;
        st.k.flushes.push(InFlightFlush {
            id,
            proc,
            node,
            what: FlushWhat::File { mark, meta },
        });
        st.ev(EventKind::FlushStart, None, proc, node, u64::from(meta), id);
        ctx.point(CallKind::Sync, node, 1);
        let st = ctx.st();
        // The mark at the return: its start state plus what concurrent flushes cleaned meanwhile (FM-3.1).
        let FlushWhat::File { mark, .. } = st.end_flush(id).what else {
            unreachable!("simulator: a file flush's record is a file flush")
        };
        let fault = st.pick(Site::FlushFault, proc, node, u64::from(meta), 4);
        let op = if meta { Op::SyncMeta } else { Op::SyncData };
        let r = if fault == 0 {
            st.flush_succeeded(node, &mark, meta);
            st.ev(EventKind::FlushEnd, None, proc, node, u64::from(meta), 0);
            Ok(())
        } else {
            st.flush_did_fail(node, &mark);
            st.ev(EventKind::FlushEnd, None, proc, node, u64::from(meta), 1);
            Err(fail_durability(
                st,
                proc,
                embedded,
                kind.class(),
                flush_fault_kind(fault),
                op,
            ))
        };
        let code = r.as_ref().map_or_else(|f| error_code(f.kind), |_| 0);
        ctx.ret_code(CallKind::Sync, node, code);
        r
    }

    /// `sync_dir(root, dir)`; `embedded` as for [`fail_durability`].
    pub(crate) fn sync_dir_op(
        &self,
        root: &SimRoot,
        dir: Option<RelPath<'_>>,
        embedded: bool,
    ) -> Result<(), DurabilityFailure> {
        let mut ctx = self.enter(CallKind::SyncDir, root.node);
        let proc = self.proc;
        let st = ctx.st();
        let found = match st.k.ns.lookup(root.node, dir.unwrap_or(RelPath::ROOT)) {
            Ok(d) if st.k.ns.node(d).is_dir() => Ok(d),
            _ => Err(VfsErrorKind::NotFound),
        };
        let r = match found {
            Err(kind) => Err(fail_durability(
                st,
                proc,
                embedded,
                DurabilityClass::DurableName,
                kind,
                Op::SyncDir,
            )),
            // The directory-flush handle needs write access ([OS/fs §4.4.3], §5.1).
            Ok(_) if root.access == RootAccess::Read => Err(fail_durability(
                st,
                proc,
                embedded,
                DurabilityClass::DurableName,
                VfsErrorKind::AccessDenied,
                Op::SyncDir,
            )),
            Ok(d) => ctx.sync_dir_node(d, embedded),
        };
        let node = found.unwrap_or(root.node);
        let code = r.as_ref().map_or_else(|f| error_code(f.kind), |_| 0);
        ctx.ret_code(CallKind::SyncDir, node, code);
        r
    }
}

/// A directory create of `name` in `dir`, pending until `sync_dir(dir)` (FM-2.3); returns the new node.
fn mkdir_op(st: &mut State, proc: u32, dir: u64, name: String) -> u64 {
    let vol = st.k.ns.node(dir).vol;
    let node = st.k.ns.new_node(
        Kind::Dir {
            creation_durable: false,
        },
        vol,
    );
    let id = st.k.ns.push(NsOp::Create { dir, name, node });
    st.ev(EventKind::NsOp, None, proc, id, 0, node);
    node
}

/// `create_root`'s clean-up after its embedded flush failed ([OS/fs §4.1]): removes the new directory while it is still
/// named `name` in `parent` and empty. The removal may fail like any namespace operation (FM-8.2, NS-4); its error is
/// ignored, and the removal is as pending as the create it undoes.
fn remove_new_dir(ctx: &mut Ctx<'_>, parent: u64, name: &str, node: u64) {
    let proc = ctx.proc;
    let st = ctx.st();
    if st.k.ns.child(parent, name) != Some(node) || st.k.ns.cur.has_children(node) {
        return;
    }
    if attempt(ctx, &[node], ShareRetry::None, Op::Rmdir).is_err() {
        return;
    }
    let st = ctx.st();
    let id = st.k.ns.push(NsOp::Remove {
        dir: parent,
        name: name.to_owned(),
        node,
    });
    st.ev(EventKind::NsOp, None, proc, id, 1, node);
    st.k.procs[proc as usize].counters.unlinks += 1;
}

/// [F15 §3.13] and [OS/fs §4.1] (spec sync 2a): once `create_root`, `swap_dirs` or `swap_recover` has returned
/// `FlushFailed`, every further write, flush, create or namespace call of the process is a protocol violation; the
/// call's own clean-up inside it was not.
pub(crate) fn embedded_flush_ends<T>(ctx: &mut Ctx<'_>, r: &Result<T, VfsError>) {
    if matches!(r, Err(e) if e.kind == VfsErrorKind::FlushFailed) {
        let proc = ctx.proc;
        ctx.st().k.procs[proc as usize].failed_nonlazy = true;
    }
}

/// The body of both renames (§5.4, §5.5).
#[allow(clippy::too_many_arguments)] // as `SimVfs::rename_op`, plus the call context.
fn rename_in(
    ctx: &mut Ctx<'_>,
    from_root: &SimRoot,
    from: RelPath<'_>,
    to_root: &SimRoot,
    to: RelPath<'_>,
    retry: ShareRetry,
    replace: bool,
    flag: bool,
) -> Result<(), VfsError> {
    let proc = ctx.proc;
    let st = ctx.st();
    if flag {
        flag_bounded_retry(st, proc, retry, from_root.node);
    }
    if from_root.access == RootAccess::Read || to_root.access == RootAccess::Read {
        return Err(err(st, VfsErrorKind::AccessDenied, Op::Rename));
    }
    note_nonlazy_call(st, proc, from_root.node);
    let (fd, fname) =
        st.k.ns
            .parent_of(from_root.node, from)
            .map_err(|k| err(st, k, Op::Rename))?;
    let (td, tname) =
        st.k.ns
            .parent_of(to_root.node, to)
            .map_err(|k| err(st, k, Op::Rename))?;
    let node =
        st.k.ns
            .child(fd, fname)
            .ok_or_else(|| err(st, VfsErrorKind::NotFound, Op::Rename))?;
    if st.k.ns.node(node).delete_pending.is_some() {
        return Err(err(st, VfsErrorKind::DeletePending, Op::Rename));
    }
    if st.k.ns.node(node).vol != st.k.ns.node(td).vol {
        return Err(err(st, VfsErrorKind::CrossDevice, Op::Rename));
    }
    writable_volume(st, node, Op::Rename)?;
    if st.k.ns.node(node).is_dir() {
        // `rename_replace` renames files only ([OS/fs §4.8]; [F15 §5.5] "for files only"); a directory moves only by a
        // no-replace rename ([F15 §5.4]) or `swap_dirs`.
        if replace {
            return Err(err(st, VfsErrorKind::AccessDenied, Op::Rename));
        }
        if st.k.ns.is_ancestor(node, td) {
            return Err(err(st, VfsErrorKind::InvalidName, Op::Rename));
        }
    }
    let replaced = st.k.ns.child(td, tname);
    if let Some(d) = replaced {
        if d == node {
            return Ok(());
        }
        let dn = st.k.ns.node(d);
        if dn.delete_pending.is_some() {
            // FM-8.3, [OS/fs §6.4]: a rename onto a delete-pending name fails with `AlreadyExists`, `AccessDenied` or
            // `DeletePending`.
            let kind = match st.pick(Site::CreateOverPending, proc, d, 1, 3) {
                0 => VfsErrorKind::AlreadyExists,
                1 => VfsErrorKind::AccessDenied,
                _ => VfsErrorKind::DeletePending,
            };
            return Err(err(st, kind, Op::Rename));
        }
        if !replace {
            return Err(err(st, VfsErrorKind::AlreadyExists, Op::Rename));
        }
        if dn.is_dir() || dn.file().is_some_and(|f| f.sealed) {
            return Err(err(st, VfsErrorKind::AccessDenied, Op::Rename));
        }
    }
    // The source, and a replaced destination, may be held without the share mode a rename needs (FM-8.2).
    let mut held = vec![node];
    held.extend(replaced);
    attempt(ctx, &held, retry, Op::Rename)?;
    let st = ctx.st();
    let id = st.k.ns.push(NsOp::Rename {
        from: (fd, fname.to_owned()),
        to: (td, tname.to_owned()),
        node,
        replace,
        replaced,
    });
    let kind = if replace { 3 } else { 2 };
    st.ev(EventKind::NsOp, None, proc, id, kind, node);
    st.k.procs[proc as usize].counters.renames += 1;
    if let Some(d) = replaced {
        st.k.ns.gc(d);
    }
    Ok(())
}

/// The body of `unlink` ([OS/fs §4.7]; FM-8.3).
fn unlink_in(
    ctx: &mut Ctx<'_>,
    root: &SimRoot,
    rel: RelPath<'_>,
    retry: ShareRetry,
    flag: bool,
) -> Result<(), VfsError> {
    let proc = ctx.proc;
    let st = ctx.st();
    if flag {
        flag_bounded_retry(st, proc, retry, root.node);
    }
    if root.access == RootAccess::Read {
        return Err(err(st, VfsErrorKind::AccessDenied, Op::Unlink));
    }
    let (dir, name) =
        st.k.ns
            .parent_of(root.node, rel)
            .map_err(|k| err(st, k, Op::Unlink))?;
    note_nonlazy_call(st, proc, dir);
    let node =
        st.k.ns
            .child(dir, name)
            .ok_or_else(|| err(st, VfsErrorKind::NotFound, Op::Unlink))?;
    writable_volume(st, node, Op::Unlink)?;
    let n = st.k.ns.node(node);
    if n.delete_pending.is_some() {
        return Err(err(st, VfsErrorKind::DeletePending, Op::Unlink));
    }
    if n.is_dir() && st.k.ns.cur.has_children(node) {
        return Err(err(st, VfsErrorKind::NotEmpty, Op::Unlink));
    }
    attempt(ctx, &[node], retry, Op::Unlink)?;
    let st = ctx.st();
    st.k.procs[proc as usize].counters.unlinks += 1;
    if st.k.ns.node(node).open > 0 && st.pick(Site::DeletePending, proc, node, 0, 2) == 1 {
        st.k.ns.node_mut(node).delete_pending = Some((dir, name.to_owned()));
        return Ok(());
    }
    let id = st.k.ns.push(NsOp::Remove {
        dir,
        name: name.to_owned(),
        node,
    });
    st.ev(EventKind::NsOp, None, proc, id, 1, node);
    st.k.ns.gc(node);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// StoreFs

impl VfsTypes for SimVfs {
    type Root = SimRoot;
    type File = SimFile;
}

impl StoreFs for SimVfs {
    fn open_root(
        &self,
        dir: &Path,
        role: RootRole,
        access: RootAccess,
    ) -> Result<SimRoot, VfsError> {
        let mut ctx = self.enter(CallKind::OpenRoot, 0);
        let st = ctx.st();
        let r = match st.k.ns.lookup_abs(dir) {
            Ok(n) if st.k.ns.node(n).is_dir() => Ok(SimRoot {
                node: n,
                role,
                access,
            }),
            Ok(_) => Err(err(st, VfsErrorKind::NotFound, Op::Open)),
            Err(k) => Err(err(st, k, Op::Open)),
        };
        ret(
            &mut ctx,
            CallKind::OpenRoot,
            r.as_ref().map_or(0, |r| r.node),
            &r,
        );
        r
    }

    fn create_root(&self, dir: &Path, role: RootRole) -> Result<SimRoot, VfsError> {
        let mut ctx = self.enter(CallKind::CreateRoot, 0);
        let proc = self.proc;
        let r = (|| {
            let st = ctx.st();
            let parent = match dir.parent() {
                Some(p) => st.k.ns.lookup_abs(p).map_err(|k| err(st, k, Op::Mkdir))?,
                None => return Err(err(st, VfsErrorKind::InvalidName, Op::Mkdir)),
            };
            if !st.k.ns.node(parent).is_dir() {
                return Err(err(st, VfsErrorKind::NotFound, Op::Mkdir));
            }
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| err(st, VfsErrorKind::InvalidName, Op::Mkdir))?
                .to_owned();
            RelPath::new(&name).map_err(|_| err(st, VfsErrorKind::InvalidName, Op::Mkdir))?;
            writable_volume(st, parent, Op::Mkdir)?;
            if st.k.ns.child(parent, &name).is_some() {
                return Err(err(st, VfsErrorKind::AlreadyExists, Op::Mkdir));
            }
            note_nonlazy_call(st, proc, parent);
            st.k.procs[proc as usize].counters.creates += 1;
            // A failed directory creation leaves the namespace unchanged, or (fault 2) the new directory in place, empty
            // ([F15 §5.2], NS-4, spec sync 2a).
            let fault = st.pick(Site::CreateFault, proc, parent, 0, 3);
            if fault != 0 {
                if fault == 2 {
                    mkdir_op(st, proc, parent, name);
                }
                return Err(err(st, VfsErrorKind::DiskFull, Op::Mkdir));
            }
            let node = mkdir_op(st, proc, parent, name.clone());
            if let Err(f) = ctx.sync_dir_node(parent, true) {
                // [OS/fs §4.1]: a failed embedded `durable-name` removes the new, empty directory (the removal's own
                // error ignored) and is `FlushFailed` with the flush's code and call.
                remove_new_dir(&mut ctx, parent, &name, node);
                return Err(f.embedded());
            }
            Ok(SimRoot {
                node,
                role,
                access: RootAccess::ReadWrite,
            })
        })();
        embedded_flush_ends(&mut ctx, &r);
        ret(
            &mut ctx,
            CallKind::CreateRoot,
            r.as_ref().map_or(0, |r| r.node),
            &r,
        );
        r
    }

    fn open(
        &self,
        root: &SimRoot,
        rel: RelPath<'_>,
        access: Access,
        _hint: OpenHint,
    ) -> Result<SimFile, VfsError> {
        let mut ctx = self.enter(CallKind::Open, root.node);
        let proc = self.proc;
        let r = (|| {
            let st = ctx.st();
            let node =
                st.k.ns
                    .lookup(root.node, rel)
                    .map_err(|k| err(st, k, Op::Open))?;
            let n = st.k.ns.node(node);
            if n.is_dir() {
                return Err(open_dir_err(st));
            }
            if n.delete_pending.is_some() {
                return Err(err(st, VfsErrorKind::DeletePending, Op::Open));
            }
            if access == Access::ReadWrite {
                if root.access == RootAccess::Read || n.file().is_some_and(|f| f.sealed) {
                    return Err(err(st, VfsErrorKind::AccessDenied, Op::Open));
                }
                writable_volume(st, node, Op::Open)?;
            }
            sharing_check(st, proc, node).map_err(|k| err(st, k, Op::Open))?;
            let h = st.open_handle(node, proc, false);
            st.k.procs[proc as usize].counters.opens += 1;
            Ok(SimFile {
                sh: Arc::clone(&self.sh),
                handle: h,
                node,
                proc,
                access,
            })
        })();
        ret(
            &mut ctx,
            CallKind::Open,
            r.as_ref().map_or(0, |f| f.node),
            &r,
        );
        r
    }

    fn create_new(&self, root: &SimRoot, rel: RelPath<'_>) -> Result<SimFile, VfsError> {
        let mut ctx = self.enter(CallKind::CreateNew, root.node);
        let proc = self.proc;
        let r = (|| {
            let st = ctx.st();
            if root.access == RootAccess::Read {
                return Err(err(st, VfsErrorKind::AccessDenied, Op::Open));
            }
            let (dir, name) =
                st.k.ns
                    .parent_of(root.node, rel)
                    .map_err(|k| err(st, k, Op::Open))?;
            let vol = st.k.ns.node(dir).vol;
            writable_volume(st, dir, Op::Open)?;
            note_nonlazy_call(st, proc, dir);
            if let Some(existing) = st.k.ns.child(dir, name) {
                if st.k.ns.node(existing).delete_pending.is_some() {
                    // FM-8.3, [OS/fs §6.4]: `AlreadyExists`, `AccessDenied` or `DeletePending`.
                    let kind = match st.pick(Site::CreateOverPending, proc, existing, 0, 3) {
                        0 => VfsErrorKind::AlreadyExists,
                        1 => VfsErrorKind::AccessDenied,
                        _ => VfsErrorKind::DeletePending,
                    };
                    return Err(err(st, kind, Op::Open));
                }
                return Err(err(st, VfsErrorKind::AlreadyExists, Op::Open));
            }
            st.k.procs[proc as usize].counters.creates += 1;
            let fault = st.pick(Site::CreateFault, proc, dir, 0, 3);
            if fault != 0 {
                if fault == 2 {
                    let node = st.k.ns.new_node(Kind::File(Box::default()), vol);
                    let id = st.k.ns.push(NsOp::Create {
                        dir,
                        name: name.to_owned(),
                        node,
                    });
                    st.ev(EventKind::NsOp, None, proc, id, 0, node);
                }
                return Err(err(st, VfsErrorKind::DiskFull, Op::Open));
            }
            let node = st.k.ns.new_node(Kind::File(Box::default()), vol);
            let id = st.k.ns.push(NsOp::Create {
                dir,
                name: name.to_owned(),
                node,
            });
            st.ev(EventKind::NsOp, None, proc, id, 0, node);
            let h = st.open_handle(node, proc, false);
            Ok(SimFile {
                sh: Arc::clone(&self.sh),
                handle: h,
                node,
                proc,
                access: Access::ReadWrite,
            })
        })();
        ret(
            &mut ctx,
            CallKind::CreateNew,
            r.as_ref().map_or(0, |f| f.node),
            &r,
        );
        r
    }

    fn create_dir(&self, root: &SimRoot, rel: RelPath<'_>) -> Result<(), VfsError> {
        let mut ctx = self.enter(CallKind::CreateDir, root.node);
        let proc = self.proc;
        let r = (|| {
            let st = ctx.st();
            if root.access == RootAccess::Read {
                return Err(err(st, VfsErrorKind::AccessDenied, Op::Mkdir));
            }
            let (dir, name) =
                st.k.ns
                    .parent_of(root.node, rel)
                    .map_err(|k| err(st, k, Op::Mkdir))?;
            writable_volume(st, dir, Op::Mkdir)?;
            note_nonlazy_call(st, proc, dir);
            if st.k.ns.child(dir, name).is_some() {
                return Err(err(st, VfsErrorKind::AlreadyExists, Op::Mkdir));
            }
            st.k.procs[proc as usize].counters.creates += 1;
            // A failed directory creation leaves the namespace unchanged, or (fault 2) the new directory in place, empty
            // ([F15 §5.2], NS-4, spec sync 2a): a caller that retries meets `AlreadyExists`.
            let fault = st.pick(Site::CreateFault, proc, dir, 0, 3);
            if fault != 0 {
                if fault == 2 {
                    mkdir_op(st, proc, dir, name.to_owned());
                }
                return Err(err(st, VfsErrorKind::DiskFull, Op::Mkdir));
            }
            mkdir_op(st, proc, dir, name.to_owned());
            Ok(())
        })();
        ret(&mut ctx, CallKind::CreateDir, root.node, &r);
        r
    }

    fn remove_dir(&self, root: &SimRoot, rel: RelPath<'_>) -> Result<(), VfsError> {
        let mut ctx = self.enter(CallKind::RemoveDir, root.node);
        let proc = self.proc;
        let r = (|| {
            let st = ctx.st();
            if root.access == RootAccess::Read {
                return Err(err(st, VfsErrorKind::AccessDenied, Op::Rmdir));
            }
            let (dir, name) =
                st.k.ns
                    .parent_of(root.node, rel)
                    .map_err(|k| err(st, k, Op::Rmdir))?;
            let node =
                st.k.ns
                    .child(dir, name)
                    .ok_or_else(|| err(st, VfsErrorKind::NotFound, Op::Rmdir))?;
            if !st.k.ns.node(node).is_dir() {
                return Err(err(st, VfsErrorKind::NotFound, Op::Rmdir));
            }
            if st.k.ns.node(node).delete_pending.is_some() {
                return Err(err(st, VfsErrorKind::DeletePending, Op::Rmdir));
            }
            if st.k.ns.cur.has_children(node) {
                return Err(err(st, VfsErrorKind::NotEmpty, Op::Rmdir));
            }
            writable_volume(st, node, Op::Rmdir)?;
            note_nonlazy_call(st, proc, dir);
            // An external actor may hold the directory without the share mode (FM-8.2); `remove_dir` has no retry.
            attempt(&mut ctx, &[node], ShareRetry::None, Op::Rmdir)?;
            let st = ctx.st();
            let id = st.k.ns.push(NsOp::Remove {
                dir,
                name: name.to_owned(),
                node,
            });
            st.ev(EventKind::NsOp, None, proc, id, 1, node);
            st.k.procs[proc as usize].counters.unlinks += 1;
            Ok(())
        })();
        ret(&mut ctx, CallKind::RemoveDir, root.node, &r);
        r
    }

    fn list_dir(
        &self,
        root: &SimRoot,
        dir: Option<RelPath<'_>>,
    ) -> Result<Vec<DirEntry>, VfsError> {
        let mut ctx = self.enter(CallKind::ListDir, root.node);
        let r = (|| {
            let st = ctx.st();
            let d =
                st.k.ns
                    .lookup(root.node, dir.unwrap_or(RelPath::ROOT))
                    .map_err(|k| err(st, k, Op::List))?;
            if !st.k.ns.node(d).is_dir() {
                return Err(err(st, VfsErrorKind::NotFound, Op::List));
            }
            let out =
                st.k.ns
                    .cur
                    .children(d)
                    .map(|(name, n)| DirEntry {
                        name: EntryName::from_os_bytes(name.as_bytes()),
                        kind: if st.k.ns.node(n).is_dir() {
                            EntryKind::Dir
                        } else {
                            EntryKind::File
                        },
                    })
                    .collect();
            Ok(out)
        })();
        ret(&mut ctx, CallKind::ListDir, root.node, &r);
        r
    }

    fn read_at(&self, file: &SimFile, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::Read);
        let proc = self.proc;
        let node = file.node;
        let r = (|| {
            let st = ctx.st();
            // FM-12: injected errors, then the adversary.
            let end = offset.saturating_add(buf.len() as u64);
            let f = st.k.ns.file_mut(node);
            let mut hit = false;
            for e in &mut f.read_errors {
                if e.offset < end && offset < e.offset + e.len {
                    match &mut e.remaining {
                        None => hit = true,
                        Some(0) => {}
                        Some(n) => {
                            *n -= 1;
                            hit = true;
                        }
                    }
                    if hit {
                        break;
                    }
                }
            }
            f.read_errors.retain(|e| e.remaining != Some(0));
            if hit {
                return Err(err(st, VfsErrorKind::Io, Op::Read));
            }
            match st.pick(Site::ReadFault, proc, node, offset, 3) {
                0 => {}
                v => {
                    if v == 2 {
                        crate::world::add_read_error(st, node, offset, buf.len() as u64, None);
                    }
                    return Err(err(st, VfsErrorKind::Io, Op::Read));
                }
            }
            let n = {
                let State { ch, k, .. } = &mut *st;
                k.ns.file(node)
                    .content
                    .read(offset, buf, &mut |site, aux, ar| {
                        ch.pick(site, proc, node, aux, ar)
                    })
            };
            st.k.procs[proc as usize].counters.bytes_read += n as u64;
            // FM-4: the read's interval; writes in flight during it may show through, per sub-sector.
            let alts: Vec<(u64, Box<[u8; 512]>)> = st
                .k
                .writes
                .iter()
                .filter(|w| w.node == node)
                .flat_map(|w| st.overlap_alts(node, w.offset, w.data.as_slice(), offset, n as u64))
                .collect();
            let id = st.k.next_io;
            st.k.next_io += 1;
            st.k.reads.push(InFlightRead {
                id,
                proc,
                node,
                offset,
                len: n as u64,
                alts,
            });
            ctx.point(CallKind::Read, node, 1);
            let st = ctx.st();
            let pos = st.k.reads.iter().position(|r| r.id == id);
            if let Some(pos) = pos {
                let mut rd = st.k.reads.remove(pos);
                rd.alts.sort_by_key(|(sub, _)| *sub);
                let mut i = 0;
                while i < rd.alts.len() {
                    let sub = rd.alts[i].0;
                    let j = rd.alts[i..].iter().take_while(|(s, _)| *s == sub).count() + i;
                    let v = st.pick(Site::ConcurrentRead, proc, node, sub, 1 + (j - i) as u64);
                    if v > 0 {
                        let alt = &rd.alts[i + v as usize - 1].1;
                        let a = sub * 512;
                        let lo = a.max(offset);
                        let hi = (a + 512).min(offset + n as u64);
                        if lo < hi {
                            buf[(lo - offset) as usize..(hi - offset) as usize]
                                .copy_from_slice(&alt[(lo - a) as usize..(hi - a) as usize]);
                        }
                    }
                    i = j;
                }
            }
            Ok(n)
        })();
        ret(&mut ctx, CallKind::Read, node, &r);
        r
    }

    fn read_exact_at(&self, file: &SimFile, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        let n = self.read_at(file, offset, buf)?;
        if n < buf.len() {
            let g = self.sh.lock();
            return Err(err(&g, VfsErrorKind::UnexpectedEof, Op::Read));
        }
        Ok(())
    }

    fn write_at(&self, file: &SimFile, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::Write);
        let proc = self.proc;
        let node = file.node;
        let r = (|| {
            let st = ctx.st();
            if file.access != Access::ReadWrite {
                return Err(err(st, VfsErrorKind::AccessDenied, Op::Write));
            }
            note_nonlazy_call(st, proc, node);
            if st.k.ns.file(node).sealed {
                st.violation(ViolationKind::WriteToSealed, proc, node);
                return Err(err(st, VfsErrorKind::AccessDenied, Op::Write));
            }
            if buf.is_empty() {
                return Ok(());
            }
            // The write's interval: overlapping reads in flight may show its bytes (FM-4.1).
            let len = buf.len() as u64;
            let reads: Vec<(u64, u64, u64)> =
                st.k.reads
                    .iter()
                    .filter(|r| r.node == node)
                    .map(|r| (r.id, r.offset, r.len))
                    .collect();
            for (rid, roff, rlen) in reads {
                let alts = st.overlap_alts(node, offset, buf, roff, rlen);
                if let Some(r) = st.k.reads.iter_mut().find(|r| r.id == rid) {
                    r.alts.extend(alts);
                }
            }
            // The in-flight record: a death or a crash inside the interval applies it in part, from another thread.
            let id = st.k.next_io;
            st.k.next_io += 1;
            st.k.writes.push(InFlightWrite {
                id,
                proc,
                node,
                offset,
                data: WriteBytes::of(buf),
            });
            ctx.point(CallKind::Write, node, 1);
            let st = ctx.st();
            let pos = st.k.writes.iter().position(|w| w.id == id);
            let w = match pos {
                Some(p) => st.k.writes.remove(p),
                None => return Ok(()),
            };
            match st.pick(Site::WriteFault, proc, node, offset, 3) {
                0 => {
                    let State { ch, k, .. } = &mut *st;
                    k.ns.edit(node, |c| {
                        c.write(offset, buf, &mut |site, aux, n| {
                            ch.pick(site, proc, node, aux, n)
                        });
                    });
                    st.k.procs[proc as usize].counters.bytes_written += len;
                    Ok(())
                }
                v => {
                    let pw = PartialWrite::from_choice(st.pick(
                        Site::PartialWrite,
                        proc,
                        node,
                        offset,
                        u64::MAX,
                    ));
                    st.apply_partial(proc, &w, pw);
                    let kind = if v == 1 {
                        VfsErrorKind::DiskFull
                    } else {
                        VfsErrorKind::Io
                    };
                    Err(err(st, kind, Op::Write))
                }
            }
        })();
        ret(&mut ctx, CallKind::Write, node, &r);
        r
    }

    fn sync(&self, file: &SimFile, kind: SyncKind) -> Result<(), DurabilityFailure> {
        self.sync_op(file, kind, false)
    }

    fn sync_dir(&self, root: &SimRoot, dir: Option<RelPath<'_>>) -> Result<(), DurabilityFailure> {
        self.sync_dir_op(root, dir, false)
    }

    fn sync_group(
        &self,
        members: &[GroupMember<'_, SimRoot, SimFile>],
    ) -> Result<(), DurabilityFailure> {
        let mut ctx = self.enter(CallKind::SyncGroup, 0);
        let proc = self.proc;
        let r = sync_group_in(&mut ctx, proc, members);
        let code = r.as_ref().map_or_else(|f| error_code(f.kind), |_| 0);
        ctx.ret_code(CallKind::SyncGroup, 0, code);
        r
    }

    fn fail_stop(&self, failure: DurabilityFailure) -> ! {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let line = failure.stderr_line(st.cfg.os).to_string();
        st.stderr.push(line);
        ctx.kill_self(DeathCause::FailStop)
    }

    fn create_extent(
        &self,
        root: &SimRoot,
        rel: RelPath<'_>,
        len: u64,
        vol: &StoreVolume,
    ) -> Result<SimFile, VfsError> {
        match vol.extent_method {
            ExtentMethod::Sparse => {
                let free = self.free_space(root)?;
                if free.available < len.saturating_mul(2) {
                    return Err(VfsError::new(
                        VfsErrorKind::InsufficientSpace,
                        OsCode::NONE,
                        "free_space",
                    ));
                }
                let f = self.create_new(root, rel)?;
                self.set_len(&f, len)?;
                Ok(f)
            }
            ExtentMethod::ZeroFill | ExtentMethod::WriteZeroes => {
                let f = self.create_new(root, rel)?;
                self.zero_fill(&f, len)?;
                Ok(f)
            }
        }
    }

    fn recycle_extent(&self, file: &SimFile, len: u64, _vol: &StoreVolume) -> Result<(), VfsError> {
        self.zero_fill(file, len)
    }

    fn seal(&self, file: &SimFile) -> Result<(), VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::Seal);
        let st = ctx.st();
        let r = if file.access == Access::ReadWrite {
            st.k.ns.file_mut(file.node).sealed = true;
            Ok(())
        } else {
            Err(err(st, VfsErrorKind::AccessDenied, Op::Seal))
        };
        ret(&mut ctx, CallKind::Seal, file.node, &r);
        r
    }

    fn unlink(&self, root: &SimRoot, rel: RelPath<'_>, retry: ShareRetry) -> Result<(), VfsError> {
        self.unlink_op(root, rel, retry, true)
    }

    fn rename_noreplace(
        &self,
        from_root: &SimRoot,
        from: RelPath<'_>,
        to_root: &SimRoot,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        self.rename_op(from_root, from, to_root, to, retry, false, true)
    }

    fn rename_replace(
        &self,
        from_root: &SimRoot,
        from: RelPath<'_>,
        to_root: &SimRoot,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        self.rename_op(from_root, from, to_root, to, retry, true, true)
    }

    fn swap_dirs(
        &self,
        a_parent: &SimRoot,
        a: RelPath<'_>,
        b_parent: &SimRoot,
        b: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapOutcome, VfsError> {
        crate::swap::swap_dirs(self, a_parent, a, b_parent, b, retry)
    }

    fn swap_recover(
        &self,
        a_parent: &SimRoot,
        a: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapRecovery, VfsError> {
        crate::swap::swap_recover(self, a_parent, a, retry)
    }

    fn file_size(&self, file: &SimFile) -> Result<u64, VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::FileSize);
        let r = Ok(ctx.st().k.ns.file(file.node).content.cs());
        ret(&mut ctx, CallKind::FileSize, file.node, &r);
        r
    }

    fn identity(&self, file: &SimFile) -> Result<FileIdentity, VfsError> {
        let mut ctx = self.file_ctx(file, CallKind::Identity);
        let r = Ok(identity_of(ctx.st(), file.node));
        ret(&mut ctx, CallKind::Identity, file.node, &r);
        r
    }

    fn root_identity(&self, root: &SimRoot) -> Result<FileIdentity, VfsError> {
        let mut ctx = self.enter(CallKind::Identity, root.node);
        let st = ctx.st();
        let r = if st.k.ns.nodes.contains_key(&root.node) {
            Ok(identity_of(st, root.node))
        } else {
            Err(err(st, VfsErrorKind::NotFound, Op::Stat))
        };
        ret(&mut ctx, CallKind::Identity, root.node, &r);
        r
    }

    fn path_identity(&self, root: &SimRoot, rel: RelPath<'_>) -> Result<FileIdentity, VfsError> {
        let mut ctx = self.enter(CallKind::PathIdentity, root.node);
        let st = ctx.st();
        let r = match st.k.ns.lookup(root.node, rel) {
            Err(k) => Err(err(st, k, Op::Stat)),
            Ok(node) if st.k.ns.node(node).delete_pending.is_some() => {
                Err(err(st, VfsErrorKind::DeletePending, Op::Stat))
            }
            Ok(node) => Ok(identity_of(st, node)),
        };
        ret(&mut ctx, CallKind::PathIdentity, root.node, &r);
        r
    }

    fn free_space(&self, root: &SimRoot) -> Result<FreeSpace, VfsError> {
        let mut ctx = self.enter(CallKind::FreeSpace, root.node);
        let st = ctx.st();
        let r = match st.k.ns.nodes.get(&root.node).map(|n| n.vol) {
            None => Err(err(st, VfsErrorKind::NotFound, Op::Free)),
            Some(vol) => {
                let total = st.k.volumes[vol as usize].total_bytes;
                Ok(FreeSpace {
                    available: total.saturating_sub(st.k.ns.used_bytes(vol)),
                    total,
                })
            }
        };
        ret(&mut ctx, CallKind::FreeSpace, root.node, &r);
        r
    }

    fn advise_dontneed(&self, file: &SimFile, _offset: u64, _len: u64) {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let proc = self.proc;
        let st = ctx.st();
        if st
            .k
            .ns
            .nodes
            .get(&file.node)
            .and_then(|n| n.file())
            .is_some_and(|f| f.content.has_unflushed())
        {
            st.violation(ViolationKind::AdviseOnUnflushed, proc, file.node);
        }
    }

    fn counters(&self) -> VfsCounters {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let proc = self.proc;
        ctx.st().k.procs[proc as usize].counters
    }
}

/// The body of `sync_group` ([OS/fs §4.4.4]; FM-2.6): one group per volume, in the order of first appearance; each
/// group's members start their flushes together and share one interval and one outcome — a failure is a failed flush of
/// every file member and leaves every directory member's operations pending.
fn sync_group_in(
    ctx: &mut Ctx<'_>,
    proc: u32,
    members: &[GroupMember<'_, SimRoot, SimFile>],
) -> Result<(), DurabilityFailure> {
    // Resolve the members: (node, Some(meta) for a file | None for a directory, volume).
    let mut resolved: Vec<(u64, Option<bool>, u32)> = Vec::with_capacity(members.len());
    {
        let st = ctx.st();
        for m in members {
            match *m {
                GroupMember::File { file, kind } => {
                    assert_eq!(
                        file.proc, proc,
                        "simulator: a file handle of another process in sync_group"
                    );
                    assert_eq!(
                        file.access,
                        Access::ReadWrite,
                        "simulator: sync_group on a read-only handle"
                    );
                    let vol = st.k.ns.node(file.node).vol;
                    resolved.push((file.node, Some(kind == SyncKind::DataAndMeta), vol));
                }
                GroupMember::Dir { root, dir } => {
                    let d = match st.k.ns.lookup(root.node, dir.unwrap_or(RelPath::ROOT)) {
                        Ok(d) if st.k.ns.node(d).is_dir() => d,
                        _ => {
                            return Err(fail_durability(
                                st,
                                proc,
                                false,
                                DurabilityClass::SyncGroup,
                                VfsErrorKind::NotFound,
                                Op::SyncDir,
                            ));
                        }
                    };
                    if root.access == RootAccess::Read {
                        return Err(fail_durability(
                            st,
                            proc,
                            false,
                            DurabilityClass::SyncGroup,
                            VfsErrorKind::AccessDenied,
                            Op::SyncDir,
                        ));
                    }
                    let vol = st.k.ns.node(d).vol;
                    resolved.push((d, None, vol));
                }
            }
        }
    }
    let mac = ctx.st().cfg.os == OsTag::MacOs;
    let mut vols: Vec<u32> = Vec::new();
    for &(_, _, v) in &resolved {
        if !vols.contains(&v) {
            vols.push(v);
        }
    }
    for vol in vols {
        let group: Vec<(u64, Option<bool>)> = resolved
            .iter()
            .filter(|m| m.2 == vol)
            .map(|&(n, k, _)| (n, k))
            .collect();
        let st = ctx.st();
        let mut ids = Vec::with_capacity(group.len());
        for &(node, kind) in &group {
            note_nonlazy_call(st, proc, node);
            let id = st.k.next_io;
            st.k.next_io += 1;
            ids.push(id);
            let what = match kind {
                Some(meta) => {
                    let c = &mut st.k.procs[proc as usize].counters;
                    if meta {
                        c.sync_meta += 1;
                    } else {
                        c.sync_data += 1;
                    }
                    FlushWhat::File {
                        mark: st.k.ns.file(node).content.flush_mark(),
                        meta,
                    }
                }
                None => {
                    st.k.procs[proc as usize].counters.sync_dir += 1;
                    FlushWhat::Dir {
                        limit: st.k.ns.next_op,
                    }
                }
            };
            let limit_kind = match &what {
                FlushWhat::File { meta, .. } => u64::from(*meta),
                FlushWhat::Dir { .. } => 2,
            };
            st.k.flushes.push(InFlightFlush {
                id,
                proc,
                node,
                what,
            });
            st.ev(EventKind::FlushStart, None, proc, node, limit_kind, id);
        }
        if mac {
            // One `F_FULLFSYNC` on the last member of the volume ([OS/fs §4.4.4], §4.13).
            st.k.procs[proc as usize].counters.full_barriers += 1;
        }
        let first = group.first().map_or(0, |g| g.0);
        ctx.point(CallKind::SyncGroup, first, 1);
        let st = ctx.st();
        let flushes: Vec<InFlightFlush> =
            st.k.flushes
                .iter()
                .filter(|f| ids.contains(&f.id))
                .cloned()
                .collect();
        st.k.flushes.retain(|f| !ids.contains(&f.id));
        let fault = st.pick(Site::FlushFault, proc, first, 3, 4);
        for f in &flushes {
            match &f.what {
                FlushWhat::File { mark, meta } => {
                    if fault == 0 {
                        st.flush_succeeded(f.node, mark, *meta);
                    } else {
                        st.flush_did_fail(f.node, mark);
                    }
                    st.ev(
                        EventKind::FlushEnd,
                        None,
                        proc,
                        f.node,
                        u64::from(*meta),
                        u64::from(fault != 0),
                    );
                }
                FlushWhat::Dir { limit } => {
                    if fault == 0 {
                        for op in st.k.ns.sync_dir_ok(f.node, *limit) {
                            st.ev(EventKind::NsDurable, None, proc, op, 0, 0);
                        }
                    }
                    st.ev(
                        EventKind::FlushEnd,
                        None,
                        proc,
                        f.node,
                        2,
                        u64::from(fault != 0),
                    );
                }
            }
        }
        if fault != 0 {
            let op = if group.iter().any(|g| g.1.is_some()) {
                Op::SyncMeta
            } else {
                Op::SyncDir
            };
            return Err(fail_durability(
                st,
                proc,
                false,
                DurabilityClass::SyncGroup,
                flush_fault_kind(fault),
                op,
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Locks, SealedMaps

impl Locks for SimVfs {
    type Client = SimClient;

    fn lock_client(&self, store: &SimRoot, mode: LockMode) -> Result<SimClient, LockError> {
        locks::lock_client(&self.sh, self.proc, store, mode)
    }

    fn lock_data<'a>(&self, client: &'a SimClient) -> &'a SimFile {
        &client.data
    }

    fn try_acquire(&self, client: &mut SimClient, byte: LockByte) -> Result<Acquired, LockError> {
        locks::try_acquire(&self.sh, self.proc, client, byte)
    }

    fn acquire_within(
        &self,
        client: &mut SimClient,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError> {
        locks::acquire_within(&self.sh, self.proc, client, byte, within_ms)
    }

    fn release(&self, client: &mut SimClient, grant: Grant) {
        locks::release(&self.sh, self.proc, client, grant);
    }

    fn probe(&self, client: &SimClient, byte: LockByte) -> ProbeResult {
        locks::probe(&self.sh, self.proc, client, byte)
    }

    fn holds(&self, client: &SimClient, byte: LockByte) -> bool {
        locks::holds(&self.sh, self.proc, client, byte)
    }

    fn holds_any_role(&self) -> bool {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let proc = self.proc;
        locks::holds_any_role(ctx.st(), proc)
    }

    fn foreign_lock_check(&self, client: &SimClient) -> ProbeResult {
        locks::foreign_lock_check(&self.sh, self.proc, client)
    }
}

impl SealedMaps for SimVfs {
    type Map = SimMap;

    fn map_sealed(
        &self,
        file: &SimFile,
        expected_len: u64,
        _name: RelPath<'_>,
    ) -> Result<SimMap, MapError> {
        let mut ctx = self.file_ctx(file, CallKind::MapSealed);
        let proc = self.proc;
        let node = file.node;
        let st = ctx.st();
        let actual = st.k.ns.file(node).content.cs();
        let r = if expected_len == 0 {
            Err(MapError::Empty)
        } else if st.k.procs[proc as usize].maps >= moirai_vfs::MAP_REGISTRY_SLOTS {
            Err(MapError::RegistryFull)
        } else if actual != expected_len {
            Err(MapError::SizeMismatch {
                expected: expected_len,
                actual,
            })
        } else {
            let bytes = mapped_content(st, node, proc, actual);
            let fault = st.pick(Site::MapFault, proc, node, 0, 2) == 1;
            let h = st.open_handle(node, proc, true);
            let rec = &mut st.k.procs[proc as usize];
            rec.maps += 1;
            rec.counters.maps += 1;
            rec.counters.mapped_bytes += actual;
            Ok(SimMap {
                sh: Arc::clone(&self.sh),
                handle: h,
                node,
                proc,
                len: actual,
                fault,
                first: View {
                    bytes,
                    next: OnceLock::new(),
                },
            })
        };
        let code = match &r {
            Ok(_) => 0,
            Err(MapError::SizeMismatch { .. }) => error_code(VfsErrorKind::Stale),
            Err(MapError::Empty) => error_code(VfsErrorKind::InvalidName),
            Err(MapError::RegistryFull) => error_code(VfsErrorKind::Busy),
            Err(MapError::Io(e)) => error_code(e.kind),
        };
        ctx.ret_code(CallKind::MapSealed, node, code);
        r
    }

    fn advise(&self, _map: &SimMap, _offset: u64, _len: u64, _advice: Advice) {}
}

// ---------------------------------------------------------------------------------------------------------------------
// EnvGuard

/// The store's `tmp/` directory ([F02 §5.3]), where the `init` probe works.
const PROBE_TMP: RelPath<'static> = RelPath::literal("tmp");

/// The bound of the probe's clean-up unlinks: HOLE(OS-share-retry-ms) ([`moirai_vfs::OS_SHARE_RETRY_MS`], [OS/fs §6.3],
/// [OS/env §5] step 6); retried on Windows only, as every `Bounded` call ([`attempt`]).
const PROBE_CLEANUP_RETRY: ShareRetry = ShareRetry::Bounded {
    total_ms: moirai_vfs::OS_SHARE_RETRY_MS,
};

/// A probe file's name `tmp/probe.<nonce>` ([OS/env §5]; [F02 §6.3] `tmp-entry`: the word `probe` and the nonce in
/// decimal).
fn probe_name(nonce: u64) -> String {
    format!("tmp/probe.{nonce}")
}

/// The `RelPath` of a probe name.
fn probe_rel(name: &str) -> RelPath<'_> {
    RelPath::new(name).expect("simulator: `tmp/probe.<u64>` is a valid RelPath")
}

impl SimVfs {
    /// One nonce: a `u64` drawn with one `fill_random` call of exactly its width ([OS/README §4.6]).
    fn probe_draw(&self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_random(&mut b);
        u64::from_le_bytes(b)
    }

    /// `create_new` of a fresh `tmp/probe.<nonce>`, the nonce drawn again while the create fails with `AlreadyExists`
    /// and never equal to one of `taken` ([OS/env §5]); records the name in `made` for the clean-up.
    fn probe_create(
        &self,
        store: &SimRoot,
        taken: &[u64],
        made: &mut Vec<String>,
    ) -> Result<(u64, SimFile), VfsError> {
        loop {
            let n = self.probe_draw();
            if taken.contains(&n) {
                continue;
            }
            let name = probe_name(n);
            match self.create_new(store, probe_rel(&name)) {
                Ok(f) => {
                    made.push(name);
                    return Ok((n, f));
                }
                Err(e) if e.kind == VfsErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Steps 3–5 of [OS/env §5] with the nonce names a, b and c; `Ok(Some(refusal))` for a refused call. The probe is
    /// the one consumer of a durability failure that is not `fail_stop` (step 3): its flushes run embedded, so the
    /// clean-up that follows a refusal is no protocol violation.
    fn probe_calls(
        &self,
        store: &SimRoot,
        made: &mut Vec<String>,
    ) -> Result<Option<Refusal>, VfsError> {
        // 3. The durable-write probe on `tmp/probe.a`.
        let (a, f) = self.probe_create(store, &[], made)?;
        let page: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        self.write_at(&f, 0, &page)?;
        for step in 0..4 {
            let r = match step {
                0 => self.sync_op(&f, SyncKind::Data, true),
                1 => self.sync_op(&f, SyncKind::DataAndMeta, true),
                2 => self.sync_dir_op(store, Some(PROBE_TMP), true),
                _ => self.sync_dir_op(store, None, true),
            };
            if let Err(df) = r {
                return Ok(Some(Refusal::NoDurableFlush {
                    call: df.call,
                    os: df.os,
                }));
            }
        }
        // 4. The lock probe at 2^62 on `tmp/probe.a`; the simulated kernel always has byte-range locks.
        {
            let mut g = self.sh.lock();
            locks::probe_file_lock(&mut g, self.proc, f.node);
        }
        drop(f);
        // 5. The rename probe: `create_new(tmp/probe.c)`; `tmp/probe.a → tmp/probe.b` must succeed, b drawn again (b ≠ a,
        //    c) while the rename meets `AlreadyExists`; `tmp/probe.b → tmp/probe.c` must fail with `AlreadyExists`.
        let (c, fc) = self.probe_create(store, &[a], made)?;
        drop(fc);
        let (name_a, name_c) = (probe_name(a), probe_name(c));
        let name_b = loop {
            let b = self.probe_draw();
            if b == a || b == c {
                continue;
            }
            let name_b = probe_name(b);
            match self.rename_noreplace(
                store,
                probe_rel(&name_a),
                store,
                probe_rel(&name_b),
                ShareRetry::None,
            ) {
                Ok(()) => {
                    made.push(name_b.clone());
                    break name_b;
                }
                Err(e) if e.kind == VfsErrorKind::AlreadyExists => {}
                Err(e) if e.kind == VfsErrorKind::Unsupported => {
                    return Ok(Some(Refusal::NoNoReplaceRename { os: e.os }));
                }
                Err(e) => return Err(e),
            }
        };
        match self.rename_noreplace(
            store,
            probe_rel(&name_b),
            store,
            probe_rel(&name_c),
            ShareRetry::None,
        ) {
            Ok(()) => Ok(Some(Refusal::NoNoReplaceRename { os: OsCode::NONE })),
            Err(e) if e.kind == VfsErrorKind::AlreadyExists => Ok(None),
            Err(e) if e.kind == VfsErrorKind::Unsupported => {
                Ok(Some(Refusal::NoNoReplaceRename { os: e.os }))
            }
            Err(e) => Err(e),
        }
    }
}

impl EnvGuard for SimVfs {
    fn classify(&self, store: &SimRoot, _depth: ClassifyDepth) -> Result<Classification, VfsError> {
        let mut ctx = self.enter(CallKind::Classify, store.node);
        let st = ctx.st();
        let r = match st.k.ns.nodes.get(&store.node).map(|n| n.vol) {
            None => Err(err(st, VfsErrorKind::NotFound, Op::Stat)),
            Some(vol) => {
                let p = &st.k.volumes[vol as usize];
                Ok(match &p.refusal {
                    Some(r) => Classification::Refused(r.clone()),
                    None => Classification::Local(StoreVolume {
                        fs: p.fs,
                        extent_method: p.extent_method,
                        read_only: p.read_only,
                        removable: p.removable,
                    }),
                })
            }
        };
        ret(&mut ctx, CallKind::Classify, store.node, &r);
        r
    }

    fn probe_store(&self, store: &SimRoot) -> Result<ProbeOutcome, VfsError> {
        let volume = match self.classify(store, ClassifyDepth::Full)? {
            Classification::Local(v) => v,
            Classification::Refused(r) => return Ok(ProbeOutcome::Refused(r)),
        };
        let os = match self.check_os_version() {
            Ok(v) => v,
            Err(r) => return Ok(ProbeOutcome::Refused(r)),
        };
        // `tmp/` must exist: its absence is not about the location.
        self.path_identity(store, PROBE_TMP)?;
        // Every name the probe creates is recorded, so the clean-up removes exactly the probe's own files and never a
        // concurrent prober's; leftovers of an earlier probe are `probe.<nonce>` names the orphan sweep removes.
        let mut made: Vec<String> = Vec::with_capacity(3);
        let r = self.probe_calls(store, &mut made);
        // Step 6: the clean-up, whatever the outcome. A clean-up failure never turns `Admitted` into an error or a
        // refusal ([OS/env §5] step 6, spec sync 2a): a file whose unlink still fails after the bound stays for the
        // orphan sweep, and a failed `sync_dir(tmp)` only leaves the unlinks pending. Neither goes to `fail_stop`.
        for name in &made {
            let _ = self.unlink(store, probe_rel(name), PROBE_CLEANUP_RETRY);
        }
        let _ = self.sync_dir_op(store, Some(PROBE_TMP), true);
        Ok(match r? {
            Some(refusal) => ProbeOutcome::Refused(refusal),
            None => ProbeOutcome::Admitted(ProbeReport { volume, os }),
        })
    }

    fn check_os_version(&self) -> Result<OsVersion, Refusal> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let found = st.cfg.os_version;
        let minimum = match st.cfg.os {
            OsTag::Linux => OsVersion {
                major: 5,
                minor: 10,
                build: 0,
            },
            OsTag::MacOs => OsVersion {
                major: 14,
                minor: 0,
                build: 0,
            },
            _ => OsVersion {
                major: 10,
                minor: 0,
                build: 17_134,
            },
        };
        if found < minimum {
            Err(Refusal::OsTooOld { found, minimum })
        } else {
            Ok(found)
        }
    }

    fn doctor_warnings(&self, store: &SimRoot) -> Vec<EnvWarning> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let vol = st.k.ns.nodes.get(&store.node).map_or(0, |n| n.vol);
        st.k.volumes[vol as usize].warnings.clone()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Clock, ProcHost, Entropy

impl Clock for SimVfs {
    fn wall_ms(&self) -> i64 {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let proc = self.proc;
        let st = ctx.st();
        let step = st.pick(Site::WallStep, proc, 0, 0, u64::MAX) as i64;
        let rec = &mut st.k.procs[proc as usize];
        rec.wall_offset_ms = rec.wall_offset_ms.saturating_add(step);
        let off = rec.wall_offset_ms;
        st.k.clock.wall_ms(off)
    }

    fn mono_ns(&self) -> u64 {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        ctx.st().k.clock.mono_ns
    }

    fn boot_ns(&self) -> u64 {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        ctx.st().k.clock.boot_ns
    }
}

impl Entropy for SimVfs {
    /// The next bytes of this simulated process's stream ([OS/README §4.6] "Simulator form"): bytes a test scripted
    /// first ([`crate::SimWorld::script_random`]), then the process's own generator, derived from the seed and the
    /// process index, which no other process shares. Every drawn value appears in the trace: a `Random` event with the
    /// length, the scripted count and the first 16 bytes, then `RandomMore` events for the rest of a longer draw. Not a
    /// scheduling point: the call takes no lock and does no I/O.
    fn fill_random(&self, buf: &mut [u8]) {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let (me, proc) = (ctx.me, self.proc);
        let st = ctx.st();
        let rec = &mut st.k.procs[proc as usize];
        let mut scripted = 0;
        for b in buf.iter_mut() {
            let Some(x) = rec.scripted.pop_front() else {
                break;
            };
            *b = x;
            scripted += 1;
        }
        rec.rand.fill(&mut buf[scripted..]);
        let head = &buf[..buf.len().min(16)];
        st.ev(
            EventKind::Random,
            me,
            proc,
            (buf.len() as u64 & 0xFFFF_FFFF) | (scripted as u64) << 32,
            le_padded(head),
            le_padded(head.get(8..).unwrap_or(&[])),
        );
        for more in buf.get(16..).unwrap_or(&[]).chunks(24) {
            let part = |i: usize| le_padded(more.get(i..).unwrap_or(&[]));
            st.ev(EventKind::RandomMore, me, proc, part(0), part(8), part(16));
        }
    }
}

impl ProcHost for SimVfs {
    type ParentWatch = SimParentWatch;
    type Wake = SimWake;

    fn os_tag(&self) -> OsTag {
        self.sh.lock().cfg.os
    }

    /// [OS/proc §3.2] per the simulated OS: `start` in ns since boot with `start_boot_relative` on Linux, since the Unix
    /// epoch elsewhere; no PID namespace.
    fn self_id(&self) -> ProcId {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let rec = &st.k.procs[self.proc as usize];
        let known = rec.boot_known;
        let mut flags = ProcId::START_KNOWN;
        if st.cfg.os == OsTag::Linux {
            flags |= ProcId::START_BOOT_RELATIVE;
        }
        if known {
            flags |= ProcId::BOOT_KNOWN;
        }
        ProcId {
            os: st.cfg.os as u8,
            flags,
            pid: rec.pid,
            start: rec.start_ns,
            boot_hash: if known { st.k.clock.boot_id.hash() } else { 0 },
            pidns: 0,
        }
    }

    fn parent(&self) -> Result<ParentRec, VfsError> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        match st.k.procs[self.proc as usize].parent {
            Some(p) => {
                let r = &st.k.procs[p as usize];
                Ok(ParentRec {
                    pid: r.pid,
                    start: r.start_ns,
                    start_known: true,
                })
            }
            None => Ok(ParentRec {
                pid: 0,
                start: 0,
                start_known: false,
            }),
        }
    }

    fn boot_identity(&self) -> BootIdentity {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let proc = self.proc;
        let st = ctx.st();
        if !st.k.procs[proc as usize].boot_known {
            return BootIdentity::Unknown(UnknownBoot::Denied);
        }
        if st.pick(Site::BootRead, proc, 0, 0, 2) == 1 {
            return BootIdentity::Unknown(UnknownBoot::Denied);
        }
        BootIdentity::Known(st.k.clock.boot_id)
    }

    /// The rows of [OS/proc §6.1] in order, over the simulated process table: the checker's own `ProcId` is
    /// [`ProcHost::self_id`]'s, and the lookup finds the processes of the current boot (a pid is never reused in a
    /// simulated world, so row 7 answers only a forged start).
    fn alive(&self, p: &ProcId) -> moirai_vfs::Liveness {
        use moirai_vfs::Liveness;
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        // Row 1: uninterpretable ([OS/proc §3.1]: reserved flag bits, `os` not 1–3), or another OS.
        if !(1..=3).contains(&p.os) || p.flags & 0xF0 != 0 || p.os != st.cfg.os as u8 {
            return Liveness::Unknown;
        }
        // Row 2: both boots known and different.
        let me = &st.k.procs[self.proc as usize];
        if p.flags & ProcId::BOOT_KNOWN != 0
            && me.boot_known
            && p.boot_hash != st.k.clock.boot_id.hash()
        {
            return Liveness::Unknown;
        }
        // Row 3 needs both PID namespaces known; a simulated process has none. Row 4 (a denied lookup) does not occur.
        let boot_seq = st.k.clock.boot_seq;
        match st
            .k
            .procs
            .iter()
            .find(|r| r.pid == p.pid && r.boot_seq == boot_seq)
        {
            // Row 5: no such process in this boot.
            None => Liveness::Dead,
            // Row 6: it has exited.
            Some(r) if !r.alive => Liveness::Dead,
            // Row 7: another start (the checker's start is always known).
            Some(r) if p.flags & ProcId::START_KNOWN != 0 && r.start_ns != p.start => {
                Liveness::Dead
            }
            // Row 8.
            Some(_) => Liveness::Alive,
        }
    }

    fn watch_parent(&self) -> Result<SimParentWatch, VfsError> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let parent = ctx.st().k.procs[self.proc as usize].parent;
        Ok(SimParentWatch { parent })
    }

    fn new_wake(&self) -> Result<SimWake, VfsError> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let id = st.k.next_wake;
        st.k.next_wake += 1;
        st.k.wakes.insert(id, false);
        Ok(SimWake {
            sh: Arc::clone(&self.sh),
            id,
        })
    }

    fn wait_parent_or_wake(
        &self,
        w: &SimParentWatch,
        wake: &SimWake,
    ) -> Result<WatchEvent, VfsError> {
        let mut ctx = self.enter(CallKind::WaitParent, 0);
        let r = loop {
            let st = ctx.st();
            let parent_gone = w.parent.is_none_or(|p| !st.alive(p));
            if parent_gone {
                break Ok(WatchEvent::ParentExited);
            }
            if st.k.wakes.get(&wake.id).copied().unwrap_or(false) {
                st.k.wakes.insert(wake.id, false);
                break Ok(WatchEvent::Woken);
            }
            ctx.block(Block::Parent(wake.id), None);
        };
        ret(&mut ctx, CallKind::WaitParent, 0, &r);
        r
    }

    fn parent_image(&self) -> Option<Box<str>> {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let st = ctx.st();
        let p = st.k.procs[self.proc as usize].parent?;
        Some(
            st.k.procs[p as usize]
                .name
                .to_ascii_lowercase()
                .into_boxed_str(),
        )
    }

    fn spawn_gc_child(&self, exe: &Path, args: &[&str], cwd: &Path) -> Result<u32, VfsError> {
        let mut ctx = self.enter(CallKind::SpawnGc, 0);
        let proc = self.proc;
        let st = ctx.st();
        if locks::holds_any_role(st, proc) {
            drop(ctx);
            panic!("simulator: spawn_gc_child while holding a role byte ([OS/lock §3] item 7)");
        }
        let child = st.start_proc("moirai gc", Some(proc), None);
        let pid = st.k.procs[child as usize].pid;
        st.spawns.push(SpawnRequest {
            child: SimVfs {
                sh: Arc::clone(&self.sh),
                proc: child,
            },
            exe: exe.to_path_buf(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
        });
        let r = Ok(pid);
        ret(&mut ctx, CallKind::SpawnGc, 0, &r);
        r
    }

    fn enter_background(&self) {
        let mut ctx = Ctx::quiet(&self.sh, self.proc);
        let (me, proc) = (ctx.me, self.proc);
        ctx.st().ev(EventKind::Note, me, proc, u64::MAX, 0, 0);
    }
}
