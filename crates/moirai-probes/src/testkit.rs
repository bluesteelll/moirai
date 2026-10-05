//! Test doubles shared by the unit tests: a scripted ticker and a scripted `Meter`.

use crate::arm::Ticker;
use moirai_vfs::{
    ChildPeak, ChildTicket, CpuTimes, FreeSpace, HeapCounts, Meter, MeterError, OsCode, VfsError,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fake clock whose time is shared by every handle cloned or derived from it. A handle made by `manual()` moves
/// only by `advance`; one made by `stepping(steps)` or `with_steps(steps)` also advances the shared time after each
/// of its own readings by its next step, cyclically.
#[derive(Clone, Debug, Default)]
pub struct FakeTicker {
    now: Rc<Cell<u64>>,
    steps: Rc<Vec<u64>>,
    i: Rc<Cell<usize>>,
}

impl FakeTicker {
    pub fn manual() -> FakeTicker {
        FakeTicker::default()
    }

    pub fn stepping(steps: &[u64]) -> FakeTicker {
        FakeTicker::manual().with_steps(steps)
    }

    /// A handle on the same time with its own steps.
    pub fn with_steps(&self, steps: &[u64]) -> FakeTicker {
        FakeTicker {
            now: self.now.clone(),
            steps: Rc::new(steps.to_vec()),
            i: Rc::default(),
        }
    }

    pub fn advance(&self, ns: u64) {
        self.now.set(self.now.get() + ns);
    }
}

impl Ticker for FakeTicker {
    fn now_ns(&mut self) -> u64 {
        let v = self.now.get();
        if !self.steps.is_empty() {
            let i = self.i.get();
            self.now.set(v + self.steps[i % self.steps.len()]);
            self.i.set(i + 1);
        }
        v
    }
}

pub fn meter_err(what: &'static str) -> MeterError {
    MeterError {
        os: OsCode::NONE,
        what,
    }
}

/// A scripted `Meter`: `available_physical` returns the script front to back and then repeats its last entry;
/// `reset_heap_high_water` sets the high-water mark to the live count and is counted; `cpu_times` returns its script
/// likewise.
pub struct FakeMeter {
    pub avail: Mutex<Vec<Result<u64, MeterError>>>,
    pub avail_calls: AtomicUsize,
    pub free: Result<FreeSpace, VfsError>,
    pub peak: Result<u64, MeterError>,
    pub heap: Mutex<Option<HeapCounts>>,
    pub resets: AtomicUsize,
    pub cpu: Mutex<Vec<Result<CpuTimes, MeterError>>>,
    pub cpu_calls: AtomicUsize,
}

impl FakeMeter {
    pub fn new(avail: u64) -> FakeMeter {
        FakeMeter::scripted(vec![Ok(avail)])
    }

    pub fn scripted(avail: Vec<Result<u64, MeterError>>) -> FakeMeter {
        FakeMeter {
            avail: Mutex::new(avail),
            avail_calls: AtomicUsize::new(0),
            free: Ok(FreeSpace {
                available: 100_000_000_000,
                total: 500_000_000_000,
            }),
            peak: Ok(3_000_000),
            heap: Mutex::new(None),
            resets: AtomicUsize::new(0),
            cpu: Mutex::new(vec![Err(meter_err("fake: cpu_times"))]),
            cpu_calls: AtomicUsize::new(0),
        }
    }
}

/// The entry `i` of a script, or its last entry past the end.
fn scripted<T: Clone>(script: &Mutex<Vec<T>>, calls: &AtomicUsize) -> T {
    let i = calls.fetch_add(1, Ordering::Relaxed);
    let v = script.lock().expect("fake meter lock");
    v[i.min(v.len() - 1)].clone()
}

impl Meter for FakeMeter {
    fn free_space(&self, _dir: &std::path::Path) -> Result<FreeSpace, VfsError> {
        self.free.clone()
    }

    fn private_now(&self) -> Result<u64, MeterError> {
        Err(meter_err("fake: private_now"))
    }

    fn private_peak(&self) -> Result<u64, MeterError> {
        self.peak.clone()
    }

    fn available_physical(&self) -> Result<u64, MeterError> {
        scripted(&self.avail, &self.avail_calls)
    }

    fn peak_of_child(&self, _child: &std::process::Child) -> Result<ChildPeak, MeterError> {
        Err(meter_err("fake: peak_of_child"))
    }

    fn heap_counts(&self) -> Option<HeapCounts> {
        *self.heap.lock().expect("fake meter lock")
    }

    fn reset_heap_high_water(&self) {
        self.resets.fetch_add(1, Ordering::Relaxed);
        if let Some(h) = self.heap.lock().expect("fake meter lock").as_mut() {
            h.high_water_bytes = h.live_bytes;
        }
    }

    fn child_private_now(&self, _child: &std::process::Child) -> Result<u64, MeterError> {
        Err(meter_err("fake: child_private_now"))
    }

    fn child_threads(&self, _child: &std::process::Child) -> Result<u32, MeterError> {
        Err(meter_err("fake: child_threads"))
    }

    fn cpu_times(&self, _child: Option<&std::process::Child>) -> Result<CpuTimes, MeterError> {
        scripted(&self.cpu, &self.cpu_calls)
    }

    fn prepare_child(&self, _cmd: &mut std::process::Command) -> Result<ChildTicket, MeterError> {
        Ok(ChildTicket(0))
    }

    fn bind_child(&self, _ticket: ChildTicket, _child: &std::process::Child) {}
}

/// A fresh, empty scratch directory under the system temporary directory, unique to this process and call.
pub fn scratch_dir(tag: &str) -> std::path::PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "moirai-probes-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch dir");
    d
}
