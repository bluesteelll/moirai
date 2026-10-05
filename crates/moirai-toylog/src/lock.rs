//! The toy's use of the lock bytes ([OS/lock], X-F4): one `Locks` client per store handle ([F16] P-3), the grants it
//! holds, and the in-process record that the lock-layer seeded bugs of [F16 §17.4] (L-6, L-7) and P-3 need.
//!
//! The in-process ownership decision is the seam's grant table (`moirai_vfs::GrantTable`, inside every `Vfs`); the toy
//! never re-implements it. [`ProcLocks`] is only the toy's own record of which of its handles in one process hold or
//! obtained a byte, shared by the handles of that process. Each part of the record is kept only while the seeded bug
//! that reads it is on (the holders for P-3, the waits and waited grants for L-6): with every switch off the record is
//! neither written nor read, no acquisition takes its mutex, and each acquisition is one seam call.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use moirai_vfs::{Acquired, Grant, LockByte, LockError, LockMode, ProbeResult, Vfs};

use crate::bugs::{Bug, Bugs};

/// The toy's record of the lock bytes its handles in one process hold. Cheap to clone; clones share the record. A harness
/// gives every handle of one simulated process the same value.
#[derive(Clone, Debug, Default)]
pub struct ProcLocks(Arc<Mutex<ProcReg>>);

#[derive(Debug, Default)]
struct ProcReg {
    next: u64,
    /// Byte offset → the handle that holds it.
    holders: BTreeMap<u64, u64>,
    /// Grants obtained by a wait: (byte offset, handle, monotonic ns); recorded only while L-6's seeded bug is on.
    waited: Vec<(u64, u64, u64)>,
    /// The handles inside a kernel wait: (byte offset, handle).
    waiting: Vec<(u64, u64)>,
}

impl ProcLocks {
    /// A fresh record (a process of its own).
    pub fn new() -> ProcLocks {
        ProcLocks::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut ProcReg) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

enum Held {
    Real(Grant),
    /// A grant the seeded lock bugs hand out without a kernel lock of this handle's own.
    Phantom,
}

/// One store handle's lock client and the bytes it holds.
pub struct ToyLocks<V: Vfs> {
    client: V::Client,
    held: BTreeMap<u64, Held>,
    reg: ProcLocks,
    id: u64,
    bugs: Bugs,
}

impl<V: Vfs> ToyLocks<V> {
    /// Opens `LOCK` in `root` for acquisitions.
    pub fn open(
        vfs: &V,
        root: &V::Root,
        reg: ProcLocks,
        bugs: Bugs,
    ) -> Result<ToyLocks<V>, LockError> {
        let client = vfs.lock_client(root, LockMode::Acquire)?;
        let id = reg.with(|r| {
            r.next += 1;
            r.next
        });
        Ok(ToyLocks {
            client,
            held: BTreeMap::new(),
            reg,
            id,
            bugs,
        })
    }

    /// The client's data handle on `LOCK` ([OS/lock §4]).
    pub fn data<'a>(&'a self, vfs: &V) -> &'a V::File {
        vfs.lock_data(&self.client)
    }

    /// Whether this handle holds `byte`.
    pub fn holds(&self, byte: LockByte) -> bool {
        self.held.contains_key(&byte.offset())
    }

    fn record(&mut self, byte: LockByte, h: Held, waited: bool, now: u64) {
        let (off, id) = (byte.offset(), self.id);
        // Only P-3's seeded bug reads the holders, and only L-6's the waited grants: with both off nothing is recorded.
        let holder = self.bugs.on(Bug::P03KernelPathSecondClient);
        let keep = waited && self.bugs.on(Bug::L06GrantToTwoWaiters);
        if holder || keep {
            self.reg.with(|r| {
                if holder {
                    r.holders.insert(off, id);
                }
                if keep {
                    r.waited.push((off, id, now));
                }
            });
        }
        self.held.insert(off, h);
    }

    /// A sibling handle of this process holds `byte` (the record, not the kernel).
    fn sibling_holds(&self, byte: LockByte) -> bool {
        let (off, id) = (byte.offset(), self.id);
        self.reg
            .with(|r| r.holders.get(&off).is_some_and(|&h| h != id))
    }

    /// A sibling handle of this process obtained `byte` by a wait at or after `since`.
    fn sibling_waited(&self, byte: LockByte, since: u64) -> bool {
        let (off, id) = (byte.offset(), self.id);
        self.reg.with(|r| {
            r.waited
                .iter()
                .any(|&(o, h, t)| o == off && h != id && t >= since)
        })
    }

    /// A sibling handle of this process is inside a kernel wait for `byte`.
    fn sibling_waiting(&self, byte: LockByte) -> bool {
        let (off, id) = (byte.offset(), self.id);
        self.reg
            .with(|r| r.waiting.iter().any(|&(o, h)| o == off && h != id))
    }

    /// `acquire_within`, with this handle marked as inside a kernel wait for `byte` while it runs when L-6's seeded bug,
    /// the only reader of the mark, is on.
    fn kernel_wait(
        &mut self,
        vfs: &V,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError> {
        if !self.bugs.on(Bug::L06GrantToTwoWaiters) {
            return vfs.acquire_within(&mut self.client, byte, within_ms);
        }
        let (off, id) = (byte.offset(), self.id);
        self.reg.with(|r| r.waiting.push((off, id)));
        let r = vfs.acquire_within(&mut self.client, byte, within_ms);
        self.reg.with(|r| {
            if let Some(i) = r.waiting.iter().position(|&x| x == (off, id)) {
                r.waiting.swap_remove(i);
            }
        });
        r
    }

    /// `try_acquire` ([OS/lock §4]): never waits.
    pub fn try_take(&mut self, vfs: &V, byte: LockByte) -> Result<bool, LockError> {
        match vfs.try_acquire(&mut self.client, byte)? {
            Acquired::Granted(g) => {
                let now = vfs.mono_ns();
                self.record(byte, Held::Real(g), false, now);
                Ok(true)
            }
            Acquired::Busy => Ok(false),
        }
    }

    /// `acquire_within` ([OS/lock §4], [F16] P-1, P-27, P-41): the bounded wait for the writer or the flush byte.
    pub fn wait(
        &mut self,
        vfs: &V,
        root: &V::Root,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<bool, LockError> {
        let start = vfs.mono_ns();
        // P-3's seeded bug: an in-process conflict is passed to the kernel path, which grants the byte again because
        // the process's kernel lock is already held there.
        if self.bugs.on(Bug::P03KernelPathSecondClient) {
            match vfs.try_acquire(&mut self.client, byte)? {
                Acquired::Granted(g) => {
                    self.record(byte, Held::Real(g), false, start);
                    return Ok(true);
                }
                Acquired::Busy if self.sibling_holds(byte) => {
                    self.record(byte, Held::Phantom, false, start);
                    return Ok(true);
                }
                Acquired::Busy => {}
            }
        }
        let granted = if self.bugs.on(Bug::L06GrantToTwoWaiters) && self.sibling_waiting(byte) {
            // L-6's seeded bug: a client that arrives while a sibling is inside the process's kernel wait for the byte
            // waits on that wait's outcome (here by polling the in-process table), and the grant the sibling obtains is
            // handed to it as well.
            let deadline = start.saturating_add(u64::from(within_ms) * 1_000_000);
            loop {
                match vfs.try_acquire(&mut self.client, byte)? {
                    Acquired::Granted(g) => break Some(g),
                    Acquired::Busy => {
                        if self.sibling_waited(byte, start) {
                            self.record(byte, Held::Phantom, false, start);
                            return Ok(true);
                        }
                        if vfs.mono_ns() >= deadline {
                            break None;
                        }
                    }
                }
            }
        } else {
            match self.kernel_wait(vfs, byte, within_ms)? {
                Acquired::Granted(g) => Some(g),
                Acquired::Busy => None,
            }
        };
        match granted {
            Some(g) => {
                let now = vfs.mono_ns();
                let waited = now > start;
                self.record(byte, Held::Real(g), waited, now);
                Ok(true)
            }
            None => {
                if self.bugs.on(Bug::L07LateGrantLeaked) {
                    leak_late_grant(vfs, root, byte);
                }
                Ok(false)
            }
        }
    }

    /// Releases `byte` (always unlocks in the kernel, [OS/lock §4]).
    pub fn release(&mut self, vfs: &V, byte: LockByte) {
        let off = byte.offset();
        if let Some(h) = self.held.remove(&off) {
            if self.bugs.on(Bug::P03KernelPathSecondClient) {
                let id = self.id;
                self.reg.with(|r| {
                    if r.holders.get(&off) == Some(&id) {
                        r.holders.remove(&off);
                    }
                });
            }
            if let Held::Real(g) = h {
                vfs.release(&mut self.client, g);
            }
        }
    }

    /// `probe` ([OS/lock §8]).
    pub fn probe(&self, vfs: &V, byte: LockByte) -> ProbeResult {
        vfs.probe(&self.client, byte)
    }

    /// Releases every byte this handle holds.
    pub fn release_all(&mut self, vfs: &V) {
        let bytes: Vec<u64> = self.held.keys().copied().collect();
        for off in bytes {
            if let Some(b) = LockByte::from_offset(off) {
                self.release(vfs, b);
            }
        }
    }
}

/// L-7's seeded bug: after a timed-out wait, a grant that arrives late (taken here once more, on a fresh client of the
/// same `LOCK`) is neither returned nor released: the client is leaked with its kernel lock, so the byte stays held by a
/// client whose wait returned `Busy`.
fn leak_late_grant<V: Vfs>(vfs: &V, root: &V::Root, byte: LockByte) {
    let Ok(mut late) = vfs.lock_client(root, LockMode::Acquire) else {
        return;
    };
    // The grant is dropped without `release`, and the client (which owns the kernel lock) is never dropped.
    if let Ok(Acquired::Granted(_)) = vfs.try_acquire(&mut late, byte) {
        core::mem::forget(late);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::testing::sim_store;
    use moirai_vfs::{RootAccess, RootRole, StoreFs};
    use std::path::Path;

    fn handles(
        bugs: Bugs,
    ) -> (
        moirai_vfs_sim::SimVfs,
        ProcLocks,
        ToyLocks<moirai_vfs_sim::SimVfs>,
        ToyLocks<moirai_vfs_sim::SimVfs>,
    ) {
        let c = Config::test_profile();
        let (_w, v, _) = sim_store(&c, 51);
        let root = v
            .open_root(
                Path::new(crate::testing::STORE),
                RootRole::Store,
                RootAccess::ReadWrite,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let reg = ProcLocks::new();
        let a = ToyLocks::open(&v, &root, reg.clone(), bugs).unwrap_or_else(|e| panic!("{e}"));
        let b = ToyLocks::open(&v, &root, reg.clone(), bugs).unwrap_or_else(|e| panic!("{e}"));
        (v, reg, a, b)
    }

    #[test]
    fn waited_grants_are_recorded_only_for_l6() {
        let (_v, reg, mut a, _b) = handles(Bugs::NONE);
        a.record(LockByte::Writer, Held::Phantom, true, 5);
        assert!(
            reg.with(|r| r.waited.is_empty()),
            "nothing grows with every switch off"
        );
        let (_v, reg, mut a, b) = handles(Bugs::only(Bug::L06GrantToTwoWaiters));
        a.record(LockByte::Writer, Held::Phantom, true, 5);
        assert_eq!(reg.with(|r| r.waited.len()), 1);
        assert!(b.sibling_waited(LockByte::Writer, 5));
        assert!(!b.sibling_waited(LockByte::Writer, 6));
    }

    #[test]
    fn holdings_are_recorded_per_handle_and_released() {
        // With every switch off the handle knows what it holds, and the process record stays empty.
        let (v, reg, mut a, b) = handles(Bugs::NONE);
        a.record(LockByte::Writer, Held::Phantom, false, 0);
        assert!(a.holds(LockByte::Writer));
        assert!(reg.with(|r| r.holders.is_empty()), "nothing recorded");
        assert!(!b.sibling_holds(LockByte::Writer));
        a.release(&v, LockByte::Writer);
        assert!(!a.holds(LockByte::Writer));
        // P-3's seeded bug reads the holders: they are recorded per handle and released.
        let (v, _reg, mut a, b) = handles(Bugs::only(Bug::P03KernelPathSecondClient));
        assert!(!a.holds(LockByte::Writer));
        a.record(LockByte::Writer, Held::Phantom, false, 0);
        assert!(a.holds(LockByte::Writer));
        assert!(b.sibling_holds(LockByte::Writer));
        assert!(!a.sibling_holds(LockByte::Writer));
        a.release(&v, LockByte::Writer);
        assert!(!a.holds(LockByte::Writer) && !b.sibling_holds(LockByte::Writer));
        // A real grant: taken by try, released, then free for the sibling.
        let (v, _reg, mut a, mut b) = handles(Bugs::NONE);
        assert!(
            a.try_take(&v, LockByte::Maintenance)
                .unwrap_or_else(|e| panic!("{e}"))
        );
        assert!(
            !b.try_take(&v, LockByte::Maintenance)
                .unwrap_or_else(|e| panic!("{e}"))
        );
        a.release_all(&v);
        assert!(
            b.try_take(&v, LockByte::Maintenance)
                .unwrap_or_else(|e| panic!("{e}"))
        );
    }
}
