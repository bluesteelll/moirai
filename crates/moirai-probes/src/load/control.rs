//! The controller ([MP §9.4]): from the profile's targets, the system-wide observations and the generator's own load,
//! the commands for the next interval; and the memory hold's step.
//!
//! The generator closes the loop on the machine's totals, as the protocol's memory hold does: what the rest of the
//! machine does (the OS, a measured process, typeperf) is the observed total minus the generator's own share, and the
//! generator supplies the difference between the target and that (never less than nothing). So the machine as a whole
//! follows the recorded profile whatever runs beside the generator.

use super::fixture::Targets;
use crate::units::{MB, MIB};

/// The commands for one interval.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Commands {
    /// The busy fraction of every CPU worker, 0 to 1 (one worker per logical processor).
    pub duty: f64,
    /// Disk read bytes per second the reader issues.
    pub read: f64,
    /// Disk write bytes per second the writer issues (each slice flushed to the disk).
    pub write: f64,
    /// The size of one read.
    pub read_size: u32,
    /// The size of one write.
    pub write_size: u32,
}

impl Commands {
    /// No load.
    pub const IDLE: Commands = Commands {
        duty: 0.0,
        read: 0.0,
        write: 0.0,
        read_size: super::fixture::DEFAULT_IO,
        write_size: super::fixture::DEFAULT_IO,
    };
}

/// The generator's own share of the totals over the last interval.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Own {
    /// Its processor time, percent of all logical processors.
    pub cpu: f64,
    /// The bytes per second it read from the disk.
    pub read: f64,
    /// The bytes per second it wrote to the disk.
    pub write: f64,
}

/// The weight of the newest estimate of the rest of the machine's load.
const ALPHA: f64 = 0.5;
/// The integral gain on the CPU workers' own tracking error (their sleeps overshoot by the OS timer's granularity).
const GAIN: f64 = 0.5;
/// The largest correction of the workers' duty.
const BIAS_MAX: f64 = 0.25;

/// The controller's state ([MP §9.4]).
#[derive(Clone, Debug, Default)]
pub struct Controller {
    /// Smoothed load of the rest of the machine: processor percent, read and write bytes per second.
    rest: [Option<f64>; 3],
    /// Correction of the workers' duty for their own tracking error.
    bias: f64,
    /// The processor share the last commands asked of the generator.
    asked_cpu: f64,
    /// Whether the last duty was saturated (0 or 1), which stops the integral from winding up.
    saturated: bool,
}

impl Controller {
    /// A controller that knows nothing yet.
    pub fn new() -> Controller {
        Controller::default()
    }

    /// The commands for the next interval, whose targets are `next`, from the last interval's observed totals
    /// (`None` where the sampler had no value) and the generator's own share of them.
    pub fn step(&mut self, next: &Targets, observed: [Option<f64>; 3], own: &Own) -> Commands {
        let mine = [own.cpu, own.read, own.write];
        for ((rest, obs), mine) in self.rest.iter_mut().zip(observed).zip(mine) {
            if let Some(o) = obs {
                let r = (o - mine).max(0.0);
                *rest = Some(rest.map_or(r, |old| ALPHA * r + (1.0 - ALPHA) * old));
            }
        }
        let want = |target: f64, rest: Option<f64>| (target - rest.unwrap_or(0.0)).max(0.0);
        let cpu = want(next.cpu, self.rest[0]).min(100.0);
        if !self.saturated && self.asked_cpu > 0.0 {
            self.bias =
                (self.bias + GAIN * (self.asked_cpu - own.cpu) / 100.0).clamp(-BIAS_MAX, BIAS_MAX);
        }
        let duty = if cpu > 0.0 {
            (cpu / 100.0 + self.bias).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.saturated = duty <= 0.0 || duty >= 1.0;
        self.asked_cpu = cpu;
        Commands {
            duty,
            read: want(next.read, self.rest[1]),
            write: want(next.write, self.rest[2]),
            read_size: next.read_size,
            write_size: next.write_size,
        }
    }
}

/// The hold's tolerance around its target before it allocates or frees ([MP §9.4]).
pub const HYSTERESIS: u64 = 100 * MB;
/// The unit the memory holder allocates and frees.
pub const CHUNK: u64 = 64 * MIB;
/// The most the holder allocates in one step, so a step stays short.
pub const STEP_MAX: u64 = 1_024 * MIB;

/// One step of the memory hold.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MemoryStep {
    /// Keep what is held.
    Hold,
    /// Allocate and touch this many chunks.
    Grow(u64),
    /// Free this many chunks.
    Shrink(u64),
}

/// The memory hold ([MP §9.4]): with `available` physical bytes and `held` bytes held, allocate whole chunks while
/// more than `target` + [`HYSTERESIS`] is available (at most [`STEP_MAX`] a step and never past `max_hold`), and free
/// whole chunks while less than `target` − [`HYSTERESIS`] is.
pub fn memory_step(available: u64, held: u64, target: u64, max_hold: u64) -> MemoryStep {
    if available > target + HYSTERESIS {
        let room = max_hold.saturating_sub(held) / CHUNK;
        let want = (available - target).min(STEP_MAX).div_ceil(CHUNK);
        match want.min(room) {
            0 => MemoryStep::Hold,
            n => MemoryStep::Grow(n),
        }
    } else if available + HYSTERESIS < target {
        match (target - available).div_ceil(CHUNK).min(held / CHUNK) {
            0 => MemoryStep::Hold,
            n => MemoryStep::Shrink(n),
        }
    } else {
        MemoryStep::Hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::{LOADED_HIGH, LOADED_LOW, LOADED_TARGET};
    use proptest::prelude::*;

    fn targets(cpu: f64, read: f64, write: f64) -> Targets {
        Targets {
            cpu,
            read,
            write,
            read_size: 4_096,
            write_size: 8_192,
        }
    }

    #[test]
    fn supplies_what_the_rest_of_the_machine_does_not() {
        let mut c = Controller::new();
        // Nothing observed yet: the whole target.
        let k = c.step(&targets(40.0, 1e6, 2e6), [None; 3], &Own::default());
        assert_eq!((k.duty, k.read, k.write), (0.4, 1e6, 2e6));
        assert_eq!((k.read_size, k.write_size), (4_096, 8_192));
        // The machine showed 50 % with the generator's 40 %: the rest is 10 %.
        let own = Own {
            cpu: 40.0,
            read: 1e6,
            write: 2e6,
        };
        let k = c.step(
            &targets(40.0, 1e6, 2e6),
            [Some(50.0), Some(1.5e6), Some(2e6)],
            &own,
        );
        assert!((k.duty - 0.3).abs() < 1e-9, "{k:?}");
        assert_eq!((k.read, k.write), (0.5e6, 2e6));
        // A target below the rest asks nothing.
        let k = c.step(
            &targets(5.0, 0.0, 0.0),
            [Some(50.0), None, None],
            &Own { cpu: 30.0, ..own },
        );
        assert_eq!((k.duty, k.read, k.write), (0.0, 0.0, 0.0));
    }

    #[test]
    fn corrects_the_workers_own_shortfall() {
        let mut c = Controller::new();
        let t = targets(50.0, 0.0, 0.0);
        let mut k = c.step(&t, [None; 3], &Own::default());
        // The workers deliver 80 % of their duty; the rest of the machine is idle.
        for _ in 0..40 {
            let own = Own {
                cpu: k.duty * 80.0,
                ..Own::default()
            };
            k = c.step(&t, [Some(own.cpu), Some(0.0), Some(0.0)], &own);
        }
        assert!((k.duty * 80.0 - 50.0).abs() < 1.0, "{k:?}");
    }

    #[test]
    fn memory_steps() {
        let t = LOADED_TARGET;
        assert_eq!(memory_step(t, 0, t, u64::MAX), MemoryStep::Hold);
        assert_eq!(
            memory_step(t + HYSTERESIS, 0, t, u64::MAX),
            MemoryStep::Hold
        );
        assert_eq!(
            memory_step(t + HYSTERESIS + 1, 0, t, u64::MAX),
            MemoryStep::Grow(2)
        );
        assert_eq!(
            memory_step(t + 10_000 * MB, 0, t, u64::MAX),
            MemoryStep::Grow(16)
        );
        assert_eq!(
            memory_step(t + 10_000 * MB, 0, t, 3 * CHUNK),
            MemoryStep::Grow(3)
        );
        assert_eq!(
            memory_step(t + 10_000 * MB, 3 * CHUNK, t, 3 * CHUNK),
            MemoryStep::Hold
        );
        assert_eq!(
            memory_step(t - HYSTERESIS, 10 * CHUNK, t, u64::MAX),
            MemoryStep::Hold
        );
        assert_eq!(
            memory_step(t - HYSTERESIS - 1, 10 * CHUNK, t, u64::MAX),
            MemoryStep::Shrink(2)
        );
        assert_eq!(
            memory_step(0, 10 * CHUNK, t, u64::MAX),
            MemoryStep::Shrink(10)
        );
        assert_eq!(memory_step(0, 0, t, u64::MAX), MemoryStep::Hold);
    }

    proptest! {
        /// Against a machine whose other load is constant, the hold settles inside the loaded band, and the processor
        /// totals reach any reachable target.
        #[test]
        fn settles(base_avail in 3_000u64..13_500, rest_cpu in 0.0f64..40.0, target in 0.0f64..100.0,
                   efficiency in 0.6f64..1.0) {
            let mut held = 0;
            let base = base_avail * MB;
            for _ in 0..200 {
                let avail = base.saturating_sub(held);
                match memory_step(avail, held, LOADED_TARGET, 12_000 * MB) {
                    MemoryStep::Grow(n) => held += n * CHUNK,
                    MemoryStep::Shrink(n) => held -= n * CHUNK,
                    MemoryStep::Hold => {}
                }
            }
            let avail = base.saturating_sub(held);
            prop_assert!((LOADED_LOW..=LOADED_HIGH).contains(&avail), "{} held {}", avail, held);

            let mut c = Controller::new();
            let t = targets(target, 0.0, 0.0);
            let mut k = c.step(&t, [None; 3], &Own::default());
            let mut total = 0.0;
            for _ in 0..60 {
                let own = Own { cpu: k.duty * 100.0 * efficiency, ..Own::default() };
                total = (rest_cpu + own.cpu).min(100.0);
                k = c.step(&t, [Some(total), Some(0.0), Some(0.0)], &own);
            }
            let need = (target - rest_cpu) / 100.0;
            if target >= rest_cpu + 1.0 && need * (1.0 / efficiency - 1.0) <= BIAS_MAX - 0.01
                && need / efficiency <= 0.99 {
                prop_assert!((total - target).abs() < 2.0, "total {} target {}", total, target);
            }
            if target < rest_cpu - 0.01 {
                prop_assert_eq!(k.duty, 0.0);
            }
        }
    }
}
