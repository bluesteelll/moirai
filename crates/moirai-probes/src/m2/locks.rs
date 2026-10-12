//! Measurement 2's instruments on a child writer's `Vfs`: [`LockVfs`], a pass-through `Vfs` that logs each grant and
//! release of the writer and flush bytes with the monotonic time ([`LockEvent`], [`LockLog`]), and [`PhaseTap`], a toy
//! `Tap` that times phase 1 (the replay of the generation and the decision, [F16] P-25).
//!
//! The grant of an acquisition is logged as `(req, grant)`: `req` read right before the inner `acquire_within` or
//! `try_acquire`, `grant` right after it returned a grant; a refusal (`Busy`) and every other byte are not logged. A
//! release is logged right before the inner `release`. Every other method of every sub-trait is one delegating call.
//! Each impl is bounded by what it needs (`Locks` by `Locks + Clock`), so the logging is tested over a fake inner built
//! on `moirai_vfs::GrantTable`.

use moirai_toylog::{Note, Tap};
use moirai_vfs::{
    Acquired, Advice, BootIdentity, Classification, ClassifyDepth, Clock, DirEntry,
    DurabilityFailure, Entropy, EnvGuard, EnvWarning, FileIdentity, FreeSpace, Grant, GroupMember,
    Liveness, LockByte, LockError, LockMode, Locks, MapError, OpenHint, OsTag, OsVersion,
    ParentRec, ProbeOutcome, ProbeResult, ProcHost, ProcId, Refusal, RelPath, RootAccess, RootRole,
    SealedMaps, ShareRetry, StoreFs, StoreVolume, SwapOutcome, SwapRecovery, SyncKind, VfsCounters,
    VfsError, VfsTypes, WatchEvent,
};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// A logged lock byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Byte {
    /// The writer byte.
    Writer,
    /// The flush byte.
    Flush,
}

impl Byte {
    fn of(b: LockByte) -> Option<Byte> {
        match b {
            LockByte::Writer => Some(Byte::Writer),
            LockByte::Flush => Some(Byte::Flush),
            _ => None,
        }
    }
}

/// One logged lock event, times from the `Vfs` monotonic clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockEvent {
    /// The byte was granted: requested at `req`, granted at `grant`.
    Grant {
        /// The byte.
        byte: Byte,
        /// Right before the acquisition call.
        req: u64,
        /// Right after it returned the grant.
        grant: u64,
    },
    /// The byte was released at `t` (right before the release call).
    Release {
        /// The byte.
        byte: Byte,
        /// The time.
        t: u64,
    },
}

/// The log of one process's lock events; clones share it.
#[derive(Clone, Debug, Default)]
pub struct LockLog(Arc<Mutex<Vec<LockEvent>>>);

impl LockLog {
    fn push(&self, e: LockEvent) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(e);
    }

    /// Takes every event logged so far, in order.
    pub fn drain(&self) -> Vec<LockEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// A `Vfs` that delegates every call to `inner` and logs the writer and flush bytes' grants and releases.
#[derive(Clone, Debug, Default)]
pub struct LockVfs<V> {
    inner: V,
    log: LockLog,
}

impl<V> LockVfs<V> {
    /// The wrapper over `inner`, with an empty log.
    pub fn new(inner: V) -> LockVfs<V> {
        LockVfs {
            inner,
            log: LockLog::default(),
        }
    }

    /// The log.
    pub fn log(&self) -> &LockLog {
        &self.log
    }
}

impl<V: VfsTypes> VfsTypes for LockVfs<V> {
    type Root = V::Root;
    type File = V::File;
}

impl<V: Locks + Clock> LockVfs<V> {
    /// Times one acquisition and logs it when it granted a logged byte.
    fn timed(
        &self,
        byte: LockByte,
        acquire: impl FnOnce() -> Result<Acquired, LockError>,
    ) -> Result<Acquired, LockError> {
        let req = self.inner.mono_ns();
        let r = acquire();
        if let (Ok(Acquired::Granted(_)), Some(b)) = (&r, Byte::of(byte)) {
            let grant = self.inner.mono_ns();
            self.log.push(LockEvent::Grant {
                byte: b,
                req,
                grant,
            });
        }
        r
    }
}

impl<V: Locks + Clock> Locks for LockVfs<V> {
    type Client = V::Client;

    fn lock_client(&self, store: &V::Root, mode: LockMode) -> Result<V::Client, LockError> {
        self.inner.lock_client(store, mode)
    }

    fn lock_data<'a>(&self, client: &'a V::Client) -> &'a V::File {
        self.inner.lock_data(client)
    }

    fn try_acquire(&self, client: &mut V::Client, byte: LockByte) -> Result<Acquired, LockError> {
        self.timed(byte, || self.inner.try_acquire(client, byte))
    }

    fn acquire_within(
        &self,
        client: &mut V::Client,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError> {
        self.timed(byte, || self.inner.acquire_within(client, byte, within_ms))
    }

    fn release(&self, client: &mut V::Client, grant: Grant) {
        if let Some(b) = Byte::of(grant.byte()) {
            let t = self.inner.mono_ns();
            self.log.push(LockEvent::Release { byte: b, t });
        }
        self.inner.release(client, grant);
    }

    fn probe(&self, client: &V::Client, byte: LockByte) -> ProbeResult {
        self.inner.probe(client, byte)
    }

    fn holds(&self, client: &V::Client, byte: LockByte) -> bool {
        self.inner.holds(client, byte)
    }

    fn holds_any_role(&self) -> bool {
        self.inner.holds_any_role()
    }

    fn foreign_lock_check(&self, client: &V::Client) -> ProbeResult {
        self.inner.foreign_lock_check(client)
    }
}

impl<V: StoreFs> StoreFs for LockVfs<V> {
    fn open_root(
        &self,
        dir: &Path,
        role: RootRole,
        access: RootAccess,
    ) -> Result<V::Root, VfsError> {
        self.inner.open_root(dir, role, access)
    }

    fn create_root(&self, dir: &Path, role: RootRole) -> Result<V::Root, VfsError> {
        self.inner.create_root(dir, role)
    }

    fn open(
        &self,
        root: &V::Root,
        rel: RelPath<'_>,
        access: moirai_vfs::Access,
        hint: OpenHint,
    ) -> Result<V::File, VfsError> {
        self.inner.open(root, rel, access, hint)
    }

    fn create_new(&self, root: &V::Root, rel: RelPath<'_>) -> Result<V::File, VfsError> {
        self.inner.create_new(root, rel)
    }

    fn create_dir(&self, root: &V::Root, rel: RelPath<'_>) -> Result<(), VfsError> {
        self.inner.create_dir(root, rel)
    }

    fn remove_dir(&self, root: &V::Root, rel: RelPath<'_>) -> Result<(), VfsError> {
        self.inner.remove_dir(root, rel)
    }

    fn list_dir(
        &self,
        root: &V::Root,
        dir: Option<RelPath<'_>>,
    ) -> Result<Vec<DirEntry>, VfsError> {
        self.inner.list_dir(root, dir)
    }

    fn read_at(&self, file: &V::File, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        self.inner.read_at(file, offset, buf)
    }

    fn read_exact_at(&self, file: &V::File, offset: u64, buf: &mut [u8]) -> Result<(), VfsError> {
        self.inner.read_exact_at(file, offset, buf)
    }

    fn write_at(&self, file: &V::File, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
        self.inner.write_at(file, offset, buf)
    }

    fn sync(&self, file: &V::File, kind: SyncKind) -> Result<(), DurabilityFailure> {
        self.inner.sync(file, kind)
    }

    fn sync_dir(&self, root: &V::Root, dir: Option<RelPath<'_>>) -> Result<(), DurabilityFailure> {
        self.inner.sync_dir(root, dir)
    }

    fn sync_group(
        &self,
        members: &[GroupMember<'_, V::Root, V::File>],
    ) -> Result<(), DurabilityFailure> {
        self.inner.sync_group(members)
    }

    fn fail_stop(&self, failure: DurabilityFailure) -> ! {
        self.inner.fail_stop(failure)
    }

    fn create_extent(
        &self,
        root: &V::Root,
        rel: RelPath<'_>,
        len: u64,
        vol: &StoreVolume,
    ) -> Result<V::File, VfsError> {
        self.inner.create_extent(root, rel, len, vol)
    }

    fn recycle_extent(&self, file: &V::File, len: u64, vol: &StoreVolume) -> Result<(), VfsError> {
        self.inner.recycle_extent(file, len, vol)
    }

    fn seal(&self, file: &V::File) -> Result<(), VfsError> {
        self.inner.seal(file)
    }

    fn unlink(&self, root: &V::Root, rel: RelPath<'_>, retry: ShareRetry) -> Result<(), VfsError> {
        self.inner.unlink(root, rel, retry)
    }

    fn rename_noreplace(
        &self,
        from_root: &V::Root,
        from: RelPath<'_>,
        to_root: &V::Root,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        self.inner
            .rename_noreplace(from_root, from, to_root, to, retry)
    }

    fn rename_replace(
        &self,
        from_root: &V::Root,
        from: RelPath<'_>,
        to_root: &V::Root,
        to: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<(), VfsError> {
        self.inner
            .rename_replace(from_root, from, to_root, to, retry)
    }

    fn swap_dirs(
        &self,
        a_parent: &V::Root,
        a: RelPath<'_>,
        b_parent: &V::Root,
        b: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapOutcome, VfsError> {
        self.inner.swap_dirs(a_parent, a, b_parent, b, retry)
    }

    fn swap_recover(
        &self,
        a_parent: &V::Root,
        a: RelPath<'_>,
        retry: ShareRetry,
    ) -> Result<SwapRecovery, VfsError> {
        self.inner.swap_recover(a_parent, a, retry)
    }

    fn file_size(&self, file: &V::File) -> Result<u64, VfsError> {
        self.inner.file_size(file)
    }

    fn identity(&self, file: &V::File) -> Result<FileIdentity, VfsError> {
        self.inner.identity(file)
    }

    fn root_identity(&self, root: &V::Root) -> Result<FileIdentity, VfsError> {
        self.inner.root_identity(root)
    }

    fn path_identity(&self, root: &V::Root, rel: RelPath<'_>) -> Result<FileIdentity, VfsError> {
        self.inner.path_identity(root, rel)
    }

    fn free_space(&self, root: &V::Root) -> Result<FreeSpace, VfsError> {
        self.inner.free_space(root)
    }

    fn advise_dontneed(&self, file: &V::File, offset: u64, len: u64) {
        self.inner.advise_dontneed(file, offset, len)
    }

    fn counters(&self) -> VfsCounters {
        self.inner.counters()
    }
}

impl<V: SealedMaps> SealedMaps for LockVfs<V> {
    type Map = V::Map;

    fn map_sealed(
        &self,
        file: &V::File,
        expected_len: u64,
        name: RelPath<'_>,
    ) -> Result<V::Map, MapError> {
        self.inner.map_sealed(file, expected_len, name)
    }

    fn advise(&self, map: &V::Map, offset: u64, len: u64, advice: Advice) {
        self.inner.advise(map, offset, len, advice)
    }
}

impl<V: EnvGuard> EnvGuard for LockVfs<V> {
    fn classify(&self, store: &V::Root, depth: ClassifyDepth) -> Result<Classification, VfsError> {
        self.inner.classify(store, depth)
    }

    fn probe_store(&self, store: &V::Root) -> Result<ProbeOutcome, VfsError> {
        self.inner.probe_store(store)
    }

    fn check_os_version(&self) -> Result<OsVersion, Refusal> {
        self.inner.check_os_version()
    }

    fn doctor_warnings(&self, store: &V::Root) -> Vec<EnvWarning> {
        self.inner.doctor_warnings(store)
    }
}

impl<V: Clock> Clock for LockVfs<V> {
    fn wall_ms(&self) -> i64 {
        self.inner.wall_ms()
    }

    fn mono_ns(&self) -> u64 {
        self.inner.mono_ns()
    }

    fn boot_ns(&self) -> u64 {
        self.inner.boot_ns()
    }
}

impl<V: ProcHost> ProcHost for LockVfs<V> {
    type ParentWatch = V::ParentWatch;
    type Wake = V::Wake;

    fn os_tag(&self) -> OsTag {
        self.inner.os_tag()
    }

    fn self_id(&self) -> ProcId {
        self.inner.self_id()
    }

    fn parent(&self) -> Result<ParentRec, VfsError> {
        self.inner.parent()
    }

    fn boot_identity(&self) -> BootIdentity {
        self.inner.boot_identity()
    }

    fn alive(&self, p: &ProcId) -> Liveness {
        self.inner.alive(p)
    }

    fn watch_parent(&self) -> Result<V::ParentWatch, VfsError> {
        self.inner.watch_parent()
    }

    fn new_wake(&self) -> Result<V::Wake, VfsError> {
        self.inner.new_wake()
    }

    fn wait_parent_or_wake(
        &self,
        w: &V::ParentWatch,
        wake: &V::Wake,
    ) -> Result<WatchEvent, VfsError> {
        self.inner.wait_parent_or_wake(w, wake)
    }

    fn parent_image(&self) -> Option<Box<str>> {
        self.inner.parent_image()
    }

    fn spawn_gc_child(&self, exe: &Path, args: &[&str], cwd: &Path) -> Result<u32, VfsError> {
        self.inner.spawn_gc_child(exe, args, cwd)
    }

    fn enter_background(&self) {
        self.inner.enter_background()
    }
}

impl<V: Entropy> Entropy for LockVfs<V> {
    fn fill_random(&self, buf: &mut [u8]) {
        self.inner.fill_random(buf)
    }
}

/// A toy `Tap` that times phase 1 ([`Note::Phase1`]: the replay of the store and the decision, before the writer byte)
/// on a `Clock`; every other note is dropped, as `NoTap` drops them.
#[derive(Clone, Debug)]
pub struct PhaseTap<C> {
    clock: C,
    marks: Arc<Mutex<Vec<(bool, u64)>>>,
}

impl<C> PhaseTap<C> {
    /// A tap on `clock`.
    pub fn new(clock: C) -> PhaseTap<C> {
        PhaseTap {
            clock,
            marks: Arc::default(),
        }
    }

    /// The total time of the phase-1 runs noted since the last drain (a re-run of phase 1 adds its own), in ns.
    pub fn drain_ns(&self) -> u64 {
        let marks = std::mem::take(&mut *self.marks.lock().unwrap_or_else(PoisonError::into_inner));
        let mut total = 0;
        let mut begun = None;
        for (begin, t) in marks {
            match (begin, begun) {
                (true, _) => begun = Some(t),
                (false, Some(b)) => {
                    total += t.saturating_sub(b);
                    begun = None;
                }
                (false, None) => {}
            }
        }
        total
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Tap for PhaseTap<C> {
    fn note(&self, n: Note) {
        if let Note::Phase1(begin) = n {
            let t = self.clock.mono_ns();
            self.marks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((begin, t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moirai_vfs::{ClientId, GrantTable, KernelResult, Outcome, TableId, TryStep, WaitMode};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A manual monotonic clock.
    #[derive(Clone, Debug, Default)]
    struct ManualClock(Arc<AtomicU64>);

    impl ManualClock {
        fn advance(&self, ns: u64) {
            self.0.fetch_add(ns, Ordering::Relaxed);
        }
    }

    impl Clock for ManualClock {
        fn wall_ms(&self) -> i64 {
            0
        }

        fn mono_ns(&self) -> u64 {
            self.0.load(Ordering::Relaxed)
        }

        fn boot_ns(&self) -> u64 {
            self.mono_ns()
        }
    }

    /// The fake inner: the lock bytes over a real `GrantTable`, on a manual clock that `acquire_within` advances by its
    /// scripted wait and every release by 100 ns; `busy` makes every acquisition answer `Busy`.
    struct FakeLocks {
        table: Mutex<GrantTable>,
        clock: ManualClock,
        wait: u64,
        busy: bool,
    }

    impl FakeLocks {
        fn new(wait: u64, busy: bool) -> FakeLocks {
            FakeLocks {
                table: Mutex::new(GrantTable::new(WaitMode::CallerDriven, TableId::new(1))),
                clock: ManualClock::default(),
                wait,
                busy,
            }
        }

        fn grant(&self, c: ClientId, byte: LockByte) -> Result<Acquired, LockError> {
            if self.busy {
                return Ok(Acquired::Busy);
            }
            let mut t = self.table.lock().unwrap();
            match t.begin_try(c, byte) {
                TryStep::Busy => Ok(Acquired::Busy),
                TryStep::KernelTry { .. } => {
                    match t.end_try(c, byte, Ok(KernelResult::Granted)).0 {
                        Outcome::Granted(g) => Ok(Acquired::Granted(g)),
                        Outcome::Busy => Ok(Acquired::Busy),
                        Outcome::Error(e) => Err(e),
                    }
                }
            }
        }
    }

    impl VfsTypes for FakeLocks {
        type Root = ();
        type File = ();
    }

    impl Clock for FakeLocks {
        fn wall_ms(&self) -> i64 {
            0
        }

        fn mono_ns(&self) -> u64 {
            self.clock.mono_ns()
        }

        fn boot_ns(&self) -> u64 {
            self.clock.mono_ns()
        }
    }

    impl Locks for FakeLocks {
        type Client = ClientId;

        fn lock_client(&self, _store: &(), mode: LockMode) -> Result<ClientId, LockError> {
            Ok(self.table.lock().unwrap().register(mode))
        }

        fn lock_data<'a>(&self, _client: &'a ClientId) -> &'a () {
            &()
        }

        fn try_acquire(
            &self,
            client: &mut ClientId,
            byte: LockByte,
        ) -> Result<Acquired, LockError> {
            self.grant(*client, byte)
        }

        fn acquire_within(
            &self,
            client: &mut ClientId,
            byte: LockByte,
            _within_ms: u32,
        ) -> Result<Acquired, LockError> {
            self.clock.advance(self.wait);
            self.grant(*client, byte)
        }

        fn release(&self, client: &mut ClientId, grant: Grant) {
            let _ = self.table.lock().unwrap().release(*client, grant);
            self.clock.advance(100);
        }

        fn probe(&self, _client: &ClientId, _byte: LockByte) -> ProbeResult {
            ProbeResult::Unknown
        }

        fn holds(&self, client: &ClientId, byte: LockByte) -> bool {
            self.table.lock().unwrap().holder(byte) == Some(*client)
        }

        fn holds_any_role(&self) -> bool {
            false
        }

        fn foreign_lock_check(&self, _client: &ClientId) -> ProbeResult {
            ProbeResult::Unknown
        }
    }

    fn granted(a: Result<Acquired, LockError>) -> Grant {
        match a {
            Ok(Acquired::Granted(g)) => g,
            other => panic!("not granted: {other:?}"),
        }
    }

    /// A writer grant after a wait of 3,000 ns is logged as (req, req + 3,000); a flush grant and both releases are
    /// logged, each release before the inner release; a drained log is empty.
    #[test]
    fn grants_and_releases_of_the_writer_and_flush_bytes_are_logged() {
        let v = LockVfs::new(FakeLocks::new(3_000, false));
        v.inner.clock.advance(1_000);
        let mut c = v.lock_client(&(), LockMode::Acquire).unwrap();
        let w = granted(v.acquire_within(&mut c, LockByte::Writer, 2_000));
        assert!(v.holds(&c, LockByte::Writer));
        v.inner.clock.advance(500);
        let f = granted(v.try_acquire(&mut c, LockByte::Flush));
        v.inner.clock.advance(200);
        v.release(&mut c, f);
        assert!(
            !v.inner.holds(&c, LockByte::Flush),
            "released in the inner table"
        );
        v.release(&mut c, w);
        assert_eq!(
            v.log().drain(),
            [
                LockEvent::Grant {
                    byte: Byte::Writer,
                    req: 1_000,
                    grant: 4_000
                },
                LockEvent::Grant {
                    byte: Byte::Flush,
                    req: 4_500,
                    grant: 4_500
                },
                LockEvent::Release {
                    byte: Byte::Flush,
                    t: 4_700
                },
                LockEvent::Release {
                    byte: Byte::Writer,
                    t: 4_800
                },
            ]
        );
        assert!(v.log().drain().is_empty());
        assert_eq!(v.mono_ns(), 4_900, "the clock is the inner's");
    }

    /// Another byte (`Maintenance`) is never logged, nor is a refusal.
    #[test]
    fn other_bytes_and_refusals_are_not_logged() {
        let v = LockVfs::new(FakeLocks::new(0, false));
        let mut c = v.lock_client(&(), LockMode::Acquire).unwrap();
        let m = granted(v.try_acquire(&mut c, LockByte::Maintenance));
        v.release(&mut c, m);
        assert!(v.log().drain().is_empty());
        let busy = LockVfs::new(FakeLocks::new(0, true));
        let mut c = busy.lock_client(&(), LockMode::Acquire).unwrap();
        assert_eq!(
            busy.acquire_within(&mut c, LockByte::Writer, 10),
            Ok(Acquired::Busy)
        );
        assert_eq!(
            busy.try_acquire(&mut c, LockByte::Flush),
            Ok(Acquired::Busy)
        );
        assert!(busy.log().drain().is_empty());
    }

    /// The tap sums each phase-1 run, begin to end, re-runs included, and drops every other note.
    #[test]
    fn the_phase_tap_times_phase_one() {
        let clock = ManualClock::default();
        let tap = PhaseTap::new(clock.clone());
        clock.advance(100);
        tap.note(Note::Phase1(true));
        clock.advance(250);
        tap.note(Note::Phase3(true));
        tap.note(Note::Phase1(false));
        clock.advance(50);
        tap.note(Note::Phase1(true));
        clock.advance(50);
        tap.note(Note::Phase1(false));
        tap.note(Note::Phase3(false));
        assert_eq!(tap.drain_ns(), 300);
        assert_eq!(tap.drain_ns(), 0);
    }
}
