//! The load the generator puts on the machine ([MP §9.4]): one CPU worker per logical processor, a disk writer that
//! flushes every slice, a disk reader over a read pool, and the memory holder that keeps available physical memory at
//! its target. The workers run on their own threads and take their commands from atomics, so applying a command never
//! waits.

use super::control::{CHUNK, Commands, MemoryStep, memory_step};
use super::fixture::{MAX_IO, MIN_IO};
use crate::units::{GB, GIB};
use moirai_vfs::Meter;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What the replay loop needs from the load ([MP §9.4]).
pub trait Actuators {
    /// Applies the commands for the next interval.
    fn apply(&mut self, c: &Commands);
    /// The bytes read from and written to the disk so far.
    fn own_io(&self) -> (u64, u64);
    /// The bytes the memory holder holds now.
    fn held(&self) -> u64;
    /// The first failure of a worker, if any.
    fn failure(&self) -> Option<String>;
    /// Stops every worker and frees the held memory; the write file is removed, the read pool kept for the next run.
    fn finish(&mut self) -> Result<(), String>;
}

/// The name of the write file in the scratch directory.
pub const WRITE_FILE: &str = "loadgen-write.bin";
/// The name of the read pool in the scratch directory.
pub const READ_FILE: &str = "loadgen-read.bin";
/// The pacing slice of the disk workers: each issues its share every slice, and the writer flushes it.
pub const SLICE: Duration = Duration::from_millis(100);
/// The period of a CPU worker: busy for the duty's share of it, then asleep.
pub const CPU_PERIOD: Duration = Duration::from_millis(100);
/// How often the memory holder reads available physical memory.
pub const MEMORY_PERIOD: Duration = Duration::from_millis(250);
/// How often the memory holder touches every page it holds, so the pages stay resident.
pub const TOUCH_PERIOD: Duration = Duration::from_secs(4);
/// The default read pool: twice what the hold leaves available, so reads mostly miss the file cache.
pub const DEFAULT_READ_POOL: u64 = 4 * GIB;
/// The smallest read pool.
pub const MIN_READ_POOL: u64 = 64 << 20;
/// The span the writer cycles over.
pub const WRITE_SPAN: u64 = GIB;
/// The default ceiling of the memory holder.
pub const DEFAULT_MAX_HOLD: u64 = 12 * GB;
/// The size of a page the holder touches.
const PAGE: usize = 4_096;

/// The load's configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadConfig {
    /// The directory of the write file and the read pool.
    pub scratch: PathBuf,
    /// CPU workers: one per logical processor.
    pub workers: usize,
    /// The available physical memory the holder keeps.
    pub ram_target: u64,
    /// The most the holder holds.
    pub max_hold: u64,
    /// The read pool's size; no pool and no reader when `None`.
    pub read_pool: Option<u64>,
}

/// The state the workers share.
#[derive(Debug, Default)]
struct Shared {
    stop: AtomicBool,
    duty_ppm: AtomicU32,
    read_bps: AtomicU64,
    write_bps: AtomicU64,
    read_size: AtomicU32,
    write_size: AtomicU32,
    read_bytes: AtomicU64,
    written: AtomicU64,
    held: AtomicU64,
    failure: Mutex<Option<String>>,
}

impl Shared {
    fn fail(&self, what: &str, e: impl std::fmt::Display) {
        let mut f = self.failure.lock().unwrap_or_else(|p| p.into_inner());
        if f.is_none() {
            *f = Some(format!("{what}: {e}"));
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Relaxed)
    }
}

/// The running load ([`Actuators`] over real threads).
#[derive(Debug)]
pub struct Load {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    write_file: PathBuf,
}

/// Makes the read pool, or keeps one of the right size from an earlier run, and syncs it.
fn make_pool(path: &Path, size: u64) -> Result<(), String> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() == size) {
        return Ok(());
    }
    let mut f = File::create(path).map_err(|e| format!("{READ_FILE}: {e}"))?;
    let chunk: Vec<u8> = (0..MAX_IO as usize).map(|i| (i % 251) as u8).collect();
    let mut left = size;
    while left > 0 {
        let n = left.min(chunk.len() as u64) as usize;
        f.write_all(&chunk[..n])
            .map_err(|e| format!("{READ_FILE}: {e}"))?;
        left -= n as u64;
    }
    f.sync_all().map_err(|e| format!("{READ_FILE}: {e}"))
}

/// The sleep that ends a slice begun at `start`.
fn rest_of(period: Duration, start: Instant) {
    let rest = period.saturating_sub(start.elapsed());
    if !rest.is_zero() {
        std::thread::sleep(rest);
    }
}

/// A CPU worker: busy for `duty` of every period, asleep for the rest.
fn cpu_worker(sh: &Shared) {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    while !sh.stopped() {
        let start = Instant::now();
        let busy = CPU_PERIOD.mul_f64(f64::from(sh.duty_ppm.load(Relaxed)) / 1e6);
        while start.elapsed() < busy {
            for _ in 0..256 {
                x = std::hint::black_box(x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
            }
        }
        rest_of(CPU_PERIOD, start);
    }
}

/// The share of one slice of a byte rate, added to what is carried over; at most one second's worth (or one I/O, for
/// rates below one I/O a second) is carried.
fn slice_budget(carried: f64, bps: f64, size: usize) -> f64 {
    (carried + bps * SLICE.as_secs_f64()).min(bps.max(size as f64))
}

/// The disk writer: sequential writes over the first `span` bytes of the write file ([`WRITE_SPAN`] in a replay),
/// wrapping to its start, and flushed (`sync_data`) every slice that wrote.
fn writer(sh: &Shared, path: &Path, span: u64) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let buf = vec![0xA5u8; MAX_IO as usize];
    let (mut pos, mut budget) = (0u64, 0.0);
    while !sh.stopped() {
        let start = Instant::now();
        let size = sh.write_size.load(Relaxed).clamp(MIN_IO, MAX_IO) as usize;
        budget = slice_budget(budget, f64::from_bits(sh.write_bps.load(Relaxed)), size);
        let mut wrote = false;
        while budget >= size as f64 && start.elapsed() < SLICE {
            if pos + size as u64 > span {
                f.seek(SeekFrom::Start(0))?;
                pos = 0;
            }
            f.write_all(&buf[..size])?;
            pos += size as u64;
            budget -= size as f64;
            sh.written.fetch_add(size as u64, Relaxed);
            wrote = true;
        }
        if wrote {
            f.sync_data()?;
        }
        rest_of(SLICE, start);
    }
    Ok(())
}

/// The disk reader: sequential reads over the read pool, wrapping at its end.
fn reader(sh: &Shared, path: &Path, pool: u64) -> std::io::Result<()> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; MAX_IO as usize];
    let (mut pos, mut budget) = (0u64, 0.0);
    while !sh.stopped() {
        let start = Instant::now();
        let size = sh.read_size.load(Relaxed).clamp(MIN_IO, MAX_IO) as usize;
        budget = slice_budget(budget, f64::from_bits(sh.read_bps.load(Relaxed)), size);
        while budget >= size as f64 && start.elapsed() < SLICE {
            if pos + size as u64 > pool {
                f.seek(SeekFrom::Start(0))?;
                pos = 0;
            }
            f.read_exact(&mut buf[..size])?;
            pos += size as u64;
            budget -= size as f64;
            sh.read_bytes.fetch_add(size as u64, Relaxed);
        }
        rest_of(SLICE, start);
    }
    std::hint::black_box(&buf);
    Ok(())
}

/// Writes one byte of every page, so every page of `chunk` is resident.
fn touch(chunk: &mut [u8]) {
    for i in (0..chunk.len()).step_by(PAGE) {
        chunk[i] = chunk[i].wrapping_add(1);
    }
    std::hint::black_box(chunk);
}

/// Applies one step of the hold to `chunks`, each `chunk` bytes; returns the bytes held.
fn apply_memory(chunks: &mut Vec<Box<[u8]>>, step: MemoryStep, chunk: usize) -> u64 {
    match step {
        MemoryStep::Grow(n) => {
            for _ in 0..n {
                let mut c = vec![0u8; chunk].into_boxed_slice();
                touch(&mut c);
                chunks.push(c);
            }
        }
        MemoryStep::Shrink(n) => {
            let keep = chunks.len().saturating_sub(n as usize);
            chunks.truncate(keep);
        }
        MemoryStep::Hold => {}
    }
    (chunks.len() * chunk) as u64
}

/// The memory holder's state ([MP §9.4]): the chunks it holds, its run of failed readings, and when it last touched
/// its pages.
struct Holder {
    chunks: Vec<Box<[u8]>>,
    failures: u32,
    touched: Instant,
    target: u64,
    max_hold: u64,
}

impl Holder {
    /// The readings that fail in a row before the hold fails the load.
    const FAILURES: u32 = 3;

    fn new(target: u64, max_hold: u64, now: Instant) -> Holder {
        Holder {
            chunks: Vec::new(),
            failures: 0,
            touched: now,
            target,
            max_hold,
        }
    }

    /// The bytes held.
    fn held(&self) -> u64 {
        self.chunks.len() as u64 * CHUNK
    }

    /// One step at `now`: reads available physical memory and takes one step of [`memory_step`] (a failed reading
    /// holds, and the third in a row is the error returned); then, every [`TOUCH_PERIOD`], touches every held page.
    fn step<M: Meter>(&mut self, meter: &M, now: Instant) -> Result<(), String> {
        match meter.available_physical() {
            Ok(avail) => {
                self.failures = 0;
                let step = memory_step(avail, self.held(), self.target, self.max_hold);
                apply_memory(&mut self.chunks, step, CHUNK as usize);
            }
            Err(e) => {
                self.failures += 1;
                if self.failures >= Self::FAILURES {
                    return Err(e.to_string());
                }
            }
        }
        if now.saturating_duration_since(self.touched) >= TOUCH_PERIOD {
            self.chunks.iter_mut().for_each(|c| touch(c));
            self.touched = now;
        }
        Ok(())
    }
}

/// The memory holder ([MP §9.4]): a [`Holder`] step every [`MEMORY_PERIOD`] until the load stops or the hold fails;
/// the held memory is freed when it ends.
fn holder<M: Meter>(sh: &Shared, meter: &M, target: u64, max_hold: u64) {
    let mut h = Holder::new(target, max_hold, Instant::now());
    while !sh.stopped() {
        let start = Instant::now();
        let stepped = h.step(meter, start);
        sh.held.store(h.held(), Relaxed);
        if let Err(e) = stepped {
            sh.fail("available physical memory", e);
            break;
        }
        rest_of(MEMORY_PERIOD, start);
    }
    drop(h);
    sh.held.store(0, Relaxed);
}

impl Load {
    /// Makes the read pool (when configured) before anything runs, then starts the workers, all idle until the first
    /// [`Actuators::apply`].
    pub fn start<M: Meter>(meter: Arc<M>, cfg: &LoadConfig) -> Result<Load, String> {
        std::fs::create_dir_all(&cfg.scratch).map_err(|e| format!("the scratch directory: {e}"))?;
        let write_file = cfg.scratch.join(WRITE_FILE);
        let read_file = cfg.scratch.join(READ_FILE);
        if let Some(pool) = cfg.read_pool {
            make_pool(&read_file, pool)?;
        }
        let shared = Arc::new(Shared::default());
        let mut load = Load {
            shared,
            threads: Vec::new(),
            write_file,
        };
        let spawn = |load: &mut Load, name: &str, f: Box<dyn FnOnce(&Shared) + Send>| {
            let sh = load.shared.clone();
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(move || f(&sh))
                .map(|h| load.threads.push(h))
                .map_err(|e| format!("{name} could not be started: {e}"))
        };
        for i in 0..cfg.workers.max(1) {
            spawn(&mut load, &format!("loadgen-cpu-{i}"), Box::new(cpu_worker))?;
        }
        let wf = load.write_file.clone();
        spawn(
            &mut load,
            "loadgen-writer",
            Box::new(move |sh| {
                if let Err(e) = writer(sh, &wf, WRITE_SPAN) {
                    sh.fail(WRITE_FILE, e);
                }
            }),
        )?;
        if let Some(pool) = cfg.read_pool {
            spawn(
                &mut load,
                "loadgen-reader",
                Box::new(move |sh| {
                    if let Err(e) = reader(sh, &read_file, pool) {
                        sh.fail(READ_FILE, e);
                    }
                }),
            )?;
        }
        let (target, max_hold) = (cfg.ram_target, cfg.max_hold);
        spawn(
            &mut load,
            "loadgen-memory",
            Box::new(move |sh| holder(sh, &*meter, target, max_hold)),
        )?;
        Ok(load)
    }

    /// Stops and joins every worker.
    fn stop_threads(&mut self) -> Result<(), String> {
        self.shared.stop.store(true, Relaxed);
        let mut panicked = 0;
        for t in self.threads.drain(..) {
            if t.join().is_err() {
                panicked += 1;
            }
        }
        if panicked > 0 {
            Err(format!("{panicked} load workers panicked"))
        } else {
            Ok(())
        }
    }
}

impl Actuators for Load {
    fn apply(&mut self, c: &Commands) {
        let sh = &self.shared;
        sh.duty_ppm
            .store((c.duty.clamp(0.0, 1.0) * 1e6).round() as u32, Relaxed);
        sh.read_bps.store(c.read.max(0.0).to_bits(), Relaxed);
        sh.write_bps.store(c.write.max(0.0).to_bits(), Relaxed);
        sh.read_size.store(c.read_size, Relaxed);
        sh.write_size.store(c.write_size, Relaxed);
    }

    fn own_io(&self) -> (u64, u64) {
        (
            self.shared.read_bytes.load(Relaxed),
            self.shared.written.load(Relaxed),
        )
    }

    fn held(&self) -> u64 {
        self.shared.held.load(Relaxed)
    }

    fn failure(&self) -> Option<String> {
        self.shared
            .failure
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn finish(&mut self) -> Result<(), String> {
        let joined = self.stop_threads();
        let removed = match std::fs::remove_file(&self.write_file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{WRITE_FILE}: {e}")),
            _ => Ok(()),
        };
        match self.failure() {
            Some(f) => Err(f),
            None => joined.and(removed),
        }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            let _ = self.stop_threads();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::LOADED_TARGET;
    use crate::testkit::{FakeMeter, meter_err, scratch_dir};
    use crate::units::MB;

    #[test]
    fn budgets_carry_at_most_a_second() {
        assert_eq!(slice_budget(0.0, 1e6, 4_096), 100_000.0);
        assert_eq!(slice_budget(950_000.0, 1e6, 4_096), 1e6);
        // A rate below one I/O a second still accumulates to one I/O.
        assert_eq!(slice_budget(60_000.0, 1_000.0, 65_536), 60_100.0);
        assert_eq!(slice_budget(65_500.0, 1_000.0, 65_536), 65_536.0);
        assert_eq!(slice_budget(5.0, 0.0, 4_096), 5.0);
    }

    #[test]
    fn memory_steps_allocate_and_free_whole_chunks() {
        let mut chunks = Vec::new();
        assert_eq!(
            apply_memory(&mut chunks, MemoryStep::Grow(3), 8_192),
            24_576
        );
        assert!(
            chunks
                .iter()
                .all(|c| c[0] == 1 && c[4_096] == 1 && c[1] == 0)
        );
        assert_eq!(apply_memory(&mut chunks, MemoryStep::Hold, 8_192), 24_576);
        assert_eq!(
            apply_memory(&mut chunks, MemoryStep::Shrink(2), 8_192),
            8_192
        );
        assert_eq!(apply_memory(&mut chunks, MemoryStep::Shrink(5), 8_192), 0);
    }

    #[test]
    fn pool_is_made_once() {
        let d = scratch_dir("load-pool");
        let p = d.join(READ_FILE);
        make_pool(&p, 3 << 20).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 3 << 20);
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        make_pool(&p, 3 << 20).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
        make_pool(&p, 1 << 20).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 1 << 20);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_short_real_load_reads_and_writes() {
        // A light load for a third of a second: one worker at 10 %, 2 MB/s each way, the hold at its target.
        let d = scratch_dir("load-real");
        let cfg = LoadConfig {
            scratch: d.clone(),
            workers: 1,
            ram_target: LOADED_TARGET,
            max_hold: 0,
            read_pool: Some(2 << 20),
        };
        let mut load = Load::start(Arc::new(FakeMeter::new(LOADED_TARGET)), &cfg).unwrap();
        load.apply(&Commands {
            duty: 0.1,
            read: 2e6,
            write: 2e6,
            read_size: 65_536,
            write_size: 65_536,
        });
        std::thread::sleep(Duration::from_millis(350));
        let (read, written) = load.own_io();
        assert!(read >= 65_536 && written >= 65_536, "{read} {written}");
        assert!(
            read <= 2_000_000 && written <= 2_000_000,
            "{read} {written}"
        );
        assert_eq!(load.held(), 0);
        assert_eq!(load.failure(), None);
        load.finish().unwrap();
        assert!(!d.join(WRITE_FILE).exists());
        assert!(d.join(READ_FILE).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Runs `f` on a worker thread over a fresh [`Shared`] until `done` holds of it (or 10 s pass), then stops the
    /// worker and returns the state.
    fn run_worker(
        configure: impl FnOnce(&Shared),
        f: impl FnOnce(&Shared) -> std::io::Result<()> + Send + 'static,
        done: impl Fn(&Shared) -> bool,
    ) -> Arc<Shared> {
        let sh = Arc::new(Shared::default());
        configure(&sh);
        let worker = {
            let sh = sh.clone();
            std::thread::spawn(move || f(&sh))
        };
        let t0 = Instant::now();
        while !done(&sh) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
        }
        sh.stop.store(true, Relaxed);
        worker.join().unwrap().unwrap();
        sh
    }

    #[test]
    fn the_writer_wraps_at_its_span_and_syncs_every_slice() {
        // A 16 KiB span and 4 KiB writes at 1 MB/s: about 24 writes a slice, so the first slice wraps six times.
        let d = scratch_dir("load-writer");
        let path = d.join(WRITE_FILE);
        let span = 16 * 1_024;
        let sh = run_worker(
            |sh| {
                sh.write_bps.store(1e6f64.to_bits(), Relaxed);
                sh.write_size.store(MIN_IO, Relaxed);
            },
            {
                let path = path.clone();
                move |sh| writer(sh, &path, span)
            },
            |sh| sh.written.load(Relaxed) >= 4 * span,
        );
        let written = sh.written.load(Relaxed);
        assert!(written >= 4 * span, "{written}");
        assert_eq!(written % u64::from(MIN_IO), 0);
        // The file never grows past its span, whatever was written.
        assert_eq!(std::fs::metadata(&path).unwrap().len(), span);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_reader_wraps_at_the_end_of_its_pool() {
        let d = scratch_dir("load-reader");
        let path = d.join(READ_FILE);
        let pool = 16 * 1_024;
        make_pool(&path, pool).unwrap();
        let sh = run_worker(
            |sh| {
                sh.read_bps.store(1e6f64.to_bits(), Relaxed);
                sh.read_size.store(MIN_IO, Relaxed);
            },
            {
                let path = path.clone();
                move |sh| reader(sh, &path, pool)
            },
            |sh| sh.read_bytes.load(Relaxed) >= 4 * pool,
        );
        // Four times the pool was read without running past its end.
        assert!(sh.read_bytes.load(Relaxed) >= 4 * pool);
        assert_eq!(sh.failure.lock().unwrap().clone(), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_holder_grows_holds_shrinks_and_touches() {
        // The band is [target - 100 MB, target + 100 MB]; the hold may take one chunk.
        let t = LOADED_TARGET;
        let meter = FakeMeter::scripted(vec![
            Ok(t + 500 * MB),
            Ok(t + 500 * MB),
            Ok(t),
            Err(meter_err("fake: a lost reading")),
            Ok(t - 500 * MB),
            Ok(t - 500 * MB),
            Err(meter_err("fake: 1")),
            Err(meter_err("fake: 2")),
            Ok(t),
            Err(meter_err("fake: 3")),
            Err(meter_err("fake: 4")),
            Err(meter_err("fake: 5")),
        ]);
        let t0 = Instant::now();
        let mut h = Holder::new(t, CHUNK, t0);
        let mut held = Vec::new();
        for i in 0..12u32 {
            let r = h.step(&meter, t0 + MEMORY_PERIOD * i);
            held.push((h.held() / CHUNK, r.is_ok()));
        }
        assert_eq!(
            held,
            vec![
                (1, true), // above the band: one chunk, all max_hold allows
                (1, true), // still above, at the ceiling
                (1, true), // inside the band
                (1, true), // a failed reading holds
                (0, true), // below the band: freed
                (0, true), // nothing left to free
                (0, true),
                (0, true), // two failures in a row hold
                (0, true), // a good reading ends the run of failures
                (0, true),
                (0, true),
                (0, false), // the third failure in a row fails the hold
            ]
        );
        // Every TOUCH_PERIOD the held pages are written again.
        let meter = FakeMeter::new(t + 500 * MB);
        let mut h = Holder::new(t, CHUNK, t0);
        h.step(&meter, t0).unwrap();
        assert_eq!(h.chunks[0][0], 1);
        h.step(&meter, t0 + TOUCH_PERIOD / 2).unwrap();
        assert_eq!(h.chunks[0][0], 1);
        h.step(&meter, t0 + TOUCH_PERIOD).unwrap();
        assert_eq!(h.chunks[0][PAGE], 2);
        h.step(&meter, t0 + TOUCH_PERIOD + MEMORY_PERIOD).unwrap();
        assert_eq!(h.chunks[0][0], 2);
    }

    #[test]
    fn three_failed_readings_fail_the_load() {
        let d = scratch_dir("load-meter");
        let cfg = LoadConfig {
            scratch: d.clone(),
            workers: 1,
            ram_target: LOADED_TARGET,
            max_hold: 0,
            read_pool: None,
        };
        let meter = FakeMeter::scripted(vec![Err(meter_err("fake: no memory reading"))]);
        let mut load = Load::start(Arc::new(meter), &cfg).unwrap();
        let t0 = Instant::now();
        while load.failure().is_none() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let e = load.finish().unwrap_err();
        assert!(e.contains("no memory reading"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
