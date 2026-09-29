//! `os::map` on Windows: read-only whole-file mappings of sealed files, the mapping registry and the vectored
//! `EXCEPTION_IN_PAGE_ERROR` handler ([OS/map]; X-F6 rules 1–6).
//!
//! - `map_sealed` checks `file_size == expected_len` (the header's `total_len`), then maps the whole file with
//!   `CreateFileMappingW(PAGE_READONLY)` + `MapViewOfFile(FILE_MAP_READ, offset 0)` and closes the section at once (the
//!   view keeps it alive) ([OS/map §4, §5]).
//! - The registry is a fixed table of [`MAP_REGISTRY_SLOTS`] 64-byte entries, allocated at the first mapping and never
//!   freed; each entry is a seqlock, so the handler reads it without a lock or an allocation ([OS/map §7]). A mapping is
//!   registered before it is returned and unregistered before it is unmapped.
//! - The handler is installed once, first in the vectored chain, before the first mapping is returned. On an
//!   `EXCEPTION_IN_PAGE_ERROR` whose faulting address lies in a registered mapping it writes exactly one line — `store I/O
//!   fault in <file> at <offset>: run moirai doctor --fsck` — and ends the process with exit code 7; otherwise it passes
//!   the exception on ([OS/map §8]).

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicUsize, Ordering, fence};
use std::sync::{Mutex, PoisonError};

use moirai_vfs::{Advice, MAP_REGISTRY_SLOTS, MapError, RelPath, SealedMap, SealedMaps, StoreFs};
use windows_sys::Win32::Foundation::{EXCEPTION_IN_PAGE_ERROR, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READONLY,
    PrefetchVirtualMemory, UnmapViewOfFile, WIN32_MEMORY_RANGE_ENTRY,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::fs::{COUNTERS, OsFile, OsVfs, bump, write_and_terminate};
use super::sys::{self, Domain, last_error, owned};

/// The longest name an entry keeps ([OS/map §7]); a longer name keeps its first 40 bytes.
const NAME_MAX: usize = 40;

/// One registry entry (in memory, 64 bytes; not an on-disk layout) ([OS/map §7]).
#[repr(C)]
struct Slot {
    /// Seqlock generation: even = stable, odd = being written.
    generation: AtomicU32,
    /// Bytes of `name` in use (0–40).
    name_len: AtomicU8,
    _pad: [u8; 3],
    /// Base address of the view; 0 = entry free.
    start: AtomicUsize,
    /// Mapped length in bytes.
    len: AtomicUsize,
    /// The store-relative file name.
    name: [AtomicU8; NAME_MAX],
}

const _: () = assert!(core::mem::size_of::<Slot>() == 64);

impl Slot {
    const fn new() -> Slot {
        Slot {
            generation: AtomicU32::new(0),
            name_len: AtomicU8::new(0),
            _pad: [0; 3],
            start: AtomicUsize::new(0),
            len: AtomicUsize::new(0),
            name: [const { AtomicU8::new(0) }; NAME_MAX],
        }
    }
}

/// The registry's first entry; null until the first `map_sealed` allocates it.
static REGISTRY: AtomicPtr<Slot> = AtomicPtr::new(core::ptr::null_mut());
/// Serialises registration and unregistration (the handler takes no lock).
static WRITERS: Mutex<()> = Mutex::new(());
/// Whether the in-page handler is in the vectored chain; set (under `WRITERS`) only after a successful
/// `AddVectoredExceptionHandler`, so a failed installation is attempted again by the next `map_sealed`.
static HANDLER: AtomicBool = AtomicBool::new(false);

/// Installs the handler once per process, first in the vectored chain, before the first mapping is returned
/// ([OS/map §8.1]); called under `WRITERS`. A failed registration (a null return) is `MapError::Io` with Win32 code 8
/// (`ERROR_NOT_ENOUGH_MEMORY`, the only way the call fails), so no mapping is ever returned without the handler.
fn install_handler() -> Result<(), MapError> {
    if HANDLER.load(Ordering::Relaxed) {
        return Ok(());
    }
    // SAFETY: the handler is a valid `extern "system"` function for the life of the process; first in chain.
    let h = unsafe { AddVectoredExceptionHandler(1, Some(in_page_handler)) };
    if h.is_null() {
        return Err(io(8, "AddVectoredExceptionHandler"));
    }
    HANDLER.store(true, Ordering::Relaxed);
    Ok(())
}

/// The registry, allocated once and never freed.
fn registry() -> &'static [Slot] {
    let p = REGISTRY.load(Ordering::Acquire);
    let p = if p.is_null() {
        // Called under `WRITERS`, so only one thread allocates.
        let table: Box<[Slot]> = (0..MAP_REGISTRY_SLOTS).map(|_| Slot::new()).collect();
        let p = Box::leak(table).as_mut_ptr();
        REGISTRY.store(p, Ordering::Release);
        p
    } else {
        p
    };
    // SAFETY: `p` points to the leaked, never-freed table of exactly `MAP_REGISTRY_SLOTS` entries.
    unsafe { core::slice::from_raw_parts(p, MAP_REGISTRY_SLOTS) }
}

/// The registry as the handler sees it: empty until the first mapping.
fn registry_if_any() -> &'static [Slot] {
    let p = REGISTRY.load(Ordering::Acquire);
    if p.is_null() {
        return &[];
    }
    // SAFETY: as in `registry`: a published pointer always names the full, never-freed table.
    unsafe { core::slice::from_raw_parts(p, MAP_REGISTRY_SLOTS) }
}

/// Publishes a mapping in a free entry (under `WRITERS`).
fn register(slot: &Slot, start: usize, len: usize, name: &[u8]) {
    let g = slot.generation.load(Ordering::Relaxed);
    slot.generation.store(g.wrapping_add(1), Ordering::Relaxed);
    fence(Ordering::Release);
    let n = name.len().min(NAME_MAX);
    slot.len.store(len, Ordering::Relaxed);
    slot.name_len.store(n as u8, Ordering::Relaxed);
    for (dst, &src) in slot.name.iter().zip(&name[..n]) {
        dst.store(src, Ordering::Relaxed);
    }
    slot.start.store(start, Ordering::Relaxed);
    slot.generation.store(g.wrapping_add(2), Ordering::Release);
}

/// Frees an entry (under `WRITERS`).
fn unregister(slot: &Slot) {
    let g = slot.generation.load(Ordering::Relaxed);
    slot.generation.store(g.wrapping_add(1), Ordering::Relaxed);
    fence(Ordering::Release);
    slot.start.store(0, Ordering::Relaxed);
    slot.generation.store(g.wrapping_add(2), Ordering::Release);
}

/// A consistent reading of one entry: `(start, len, name, name_len)`, or `None` if it is free or kept changing.
fn read_slot(slot: &Slot) -> Option<(usize, usize, [u8; NAME_MAX], usize)> {
    for _ in 0..3 {
        let g1 = slot.generation.load(Ordering::Acquire);
        if g1 & 1 == 1 {
            continue;
        }
        let start = slot.start.load(Ordering::Relaxed);
        let len = slot.len.load(Ordering::Relaxed);
        let n = usize::from(slot.name_len.load(Ordering::Relaxed)).min(NAME_MAX);
        let mut name = [0u8; NAME_MAX];
        for (d, s) in name.iter_mut().zip(&slot.name) {
            *d = s.load(Ordering::Relaxed);
        }
        fence(Ordering::Acquire);
        if slot.generation.load(Ordering::Relaxed) == g1 {
            return (start != 0).then_some((start, len, name, n));
        }
    }
    None
}

/// Formats the handler's line into `buf` without allocating or `std` formatting ([OS/map §8.2] items 2 and 4).
fn fault_line(buf: &mut [u8; 160], name: &[u8], offset: usize) -> usize {
    let mut n = 0usize;
    let mut put = |b: &[u8], n: &mut usize| {
        for &x in b {
            if *n < buf.len() {
                buf[*n] = x;
                *n += 1;
            }
        }
    };
    put(b"store I/O fault in ", &mut n);
    put(name, &mut n);
    put(b" at ", &mut n);
    let mut digits = [0u8; 20];
    let mut d = digits.len();
    let mut v = offset;
    loop {
        d -= 1;
        digits[d] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    put(&digits[d..], &mut n);
    put(b": run moirai doctor --fsck\n", &mut n);
    n
}

/// The vectored exception handler ([OS/map §8.2]).
unsafe extern "system" fn in_page_handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the OS passes a valid `EXCEPTION_POINTERS` whose record pointer is valid for the handler's duration.
    let rec = unsafe { &*(*info).ExceptionRecord };
    if rec.ExceptionCode != EXCEPTION_IN_PAGE_ERROR || rec.NumberParameters < 2 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let addr = rec.ExceptionInformation[1];
    for slot in registry_if_any() {
        if let Some((start, len, name, n)) = read_slot(slot)
            && addr >= start
            && addr - start < len
        {
            let mut buf = [0u8; 160];
            let k = fault_line(&mut buf, &name[..n], addr - start);
            write_and_terminate(&buf[..k], 7);
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

/// A read-only mapping of one whole sealed file ([OS/map §3]); dropping it unregisters, then unmaps.
pub struct OsMap {
    base: *const u8,
    len: usize,
    slot: usize,
}

// SAFETY: the view is read-only and immutable by protocol for the mapping's whole life ([OS/map §2] rules 1–3); the
// pointer is only read, and unmapping happens once, in `Drop`, after every borrow of `bytes()` has ended.
unsafe impl Send for OsMap {}
// SAFETY: as above; shared readers of an immutable view need no synchronisation.
unsafe impl Sync for OsMap {}

impl core::fmt::Debug for OsMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OsMap")
            .field("len", &self.len)
            .field("slot", &self.slot)
            .finish()
    }
}

impl SealedMap for OsMap {
    fn bytes(&self) -> &[u8] {
        // SAFETY: ([OS/map §6], [80 §2.5] rule 5, [X17 §4.3]) the file is immutable for its whole life by protocol
        // (rules 1–3 of [OS/map §2]); an external writer that changes it must act against the read-only attribute; if
        // one does, every read is still in bounds of the mapping because all access is typed and bounds-checked, so
        // the effect is a wrong value or a checksum failure; truncation surfaces as `EXCEPTION_IN_PAGE_ERROR`, which
        // the handler converts into exit 7; the formal "a `&[u8]` must not change" obligation is broken only by that
        // external writer. `base` is the start of a live view of exactly `len` bytes, unmapped only in `Drop`.
        unsafe { core::slice::from_raw_parts(self.base, self.len) }
    }

    fn len(&self) -> u64 {
        self.len as u64
    }
}

impl Drop for OsMap {
    fn drop(&mut self) {
        {
            let _g = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
            unregister(&registry()[self.slot]);
        }
        // SAFETY: `base` is the view `MapViewOfFile` returned, unmapped exactly once, after it left the registry.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.base.cast_mut().cast(),
            })
        };
    }
}

fn page_size() -> usize {
    static PAGE: AtomicUsize = AtomicUsize::new(0);
    let p = PAGE.load(Ordering::Relaxed);
    if p != 0 {
        return p;
    }
    let mut si = SYSTEM_INFO::default();
    // SAFETY: `si` is a live local the call writes.
    unsafe { GetSystemInfo(&mut si) };
    let p = (si.dwPageSize as usize).max(4096);
    PAGE.store(p, Ordering::Relaxed);
    p
}

fn io(code: u32, call: &'static str) -> MapError {
    MapError::Io(sys::error(code, Domain::Store, call))
}

impl SealedMaps for OsVfs {
    type Map = OsMap;

    fn map_sealed(
        &self,
        file: &OsFile,
        expected_len: u64,
        name: RelPath<'_>,
    ) -> Result<OsMap, MapError> {
        if expected_len == 0 {
            return Err(MapError::Empty);
        }
        let actual = self.file_size(file).map_err(MapError::Io)?;
        if actual != expected_len {
            return Err(MapError::SizeMismatch {
                expected: expected_len,
                actual,
            });
        }
        let len = usize::try_from(expected_len).map_err(|_| io(8, "MapViewOfFile"))?;
        let _g = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        install_handler()?;
        let reg = registry();
        let slot = reg
            .iter()
            .position(|s| s.start.load(Ordering::Relaxed) == 0)
            .ok_or(MapError::RegistryFull)?;
        let h: HANDLE = file.raw();
        // SAFETY: `h` is a valid file handle with read access; no security attributes, whole-file size (0, 0), unnamed.
        let section = unsafe {
            CreateFileMappingW(h, core::ptr::null(), PAGE_READONLY, 0, 0, core::ptr::null())
        };
        let section = owned(section).ok_or_else(|| io(last_error(), "CreateFileMappingW"))?;
        // SAFETY: `section` is a valid read-only section of at least `len` bytes (the size check above); offset 0.
        let view = unsafe { MapViewOfFile(sys::raw(&section), FILE_MAP_READ, 0, 0, len) };
        if view.Value.is_null() {
            return Err(io(last_error(), "MapViewOfFile"));
        }
        // The view keeps the section alive; the section handle closes here.
        drop(section);
        let base = view.Value.cast::<u8>().cast_const();
        register(&reg[slot], base as usize, len, name.as_str().as_bytes());
        bump(&COUNTERS.maps, 1);
        bump(&COUNTERS.mapped_bytes, expected_len);
        Ok(OsMap { base, len, slot })
    }

    fn advise(&self, map: &OsMap, offset: u64, len: u64, advice: Advice) {
        // Windows: only `WillNeed` has a call; `Random` and `Sequential` are no-ops ([OS/map §9]).
        if advice != Advice::WillNeed || len == 0 || offset >= map.len as u64 {
            return;
        }
        let page = page_size() as u64;
        let start = offset / page * page;
        let end = offset
            .saturating_add(len)
            .div_ceil(page)
            .saturating_mul(page)
            .min(map.len as u64);
        let range = WIN32_MEMORY_RANGE_ENTRY {
            // SAFETY: `start < map.len`, so the address lies inside the live view.
            VirtualAddress: unsafe { map.base.add(start as usize) }.cast_mut().cast(),
            NumberOfBytes: (end - start) as usize,
        };
        // SAFETY: one range entry inside the live view; a hint whose failure is ignored.
        unsafe { PrefetchVirtualMemory(GetCurrentProcess(), 1, &range, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_lines_are_exact() {
        let mut b = [0u8; 160];
        let n = fault_line(&mut b, b"seg.base.7", 4096);
        assert_eq!(
            &b[..n],
            b"store I/O fault in seg.base.7 at 4096: run moirai doctor --fsck\n"
        );
        let n = fault_line(&mut b, b"x", 0);
        assert_eq!(
            &b[..n],
            b"store I/O fault in x at 0: run moirai doctor --fsck\n"
        );
        let n = fault_line(&mut b, &[b'n'; 40], usize::MAX);
        assert!(n < 160 && b[n - 1] == b'\n');
    }

    #[test]
    fn seqlock_entries() {
        let s = Slot::new();
        assert_eq!(read_slot(&s), None, "free");
        register(&s, 0x1000, 64, b"cs.0001");
        let (start, len, name, n) = read_slot(&s).unwrap();
        assert_eq!((start, len, &name[..n]), (0x1000, 64, &b"cs.0001"[..]));
        assert_eq!(s.generation.load(Ordering::Relaxed) & 1, 0);
        register(&s, 0x2000, 8, &[b'a'; 50]);
        let (_, _, _, n) = read_slot(&s).unwrap();
        assert_eq!(n, NAME_MAX, "a longer name keeps its first 40 bytes");
        unregister(&s);
        assert_eq!(read_slot(&s), None);
        // An entry caught mid-write reads as absent.
        s.generation.store(7, Ordering::Relaxed);
        assert_eq!(read_slot(&s), None);
    }
}
