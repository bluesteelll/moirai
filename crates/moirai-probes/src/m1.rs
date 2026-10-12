//! Measurement 1 (WP-52, [60 §5.2] item 1, `60-roadmap.md:757`): the CLI commit cycle on a 64 MiB log — open, append,
//! data-only flush, close — on the real `Vfs` with the toy log's production profile, the Defender close cost, and the
//! data-only-flush floor the cycle is gated against (`60-roadmap.md:788`), in the same run ([MP §4.1], [MP §4.3]).
//! It decides whether CLI commits exceed 20 ms, the trigger of the optional leader (`ARCHITECTURE-RESEARCH.md:140`),
//! judged on p99 at tier t2 (R-24 Q9). The arms are generic over [`store::Rig`]; the composition root passes
//! `store::ToyRig` over `moirai_os::OsVfs`.
//!
//! **The quantities** (R-31 Q3; names by [MP §4.1]), in run order, each with minimum tier t2:
//!
//! | Quantity | Arms (arm 0 first) | Timed region of one sample |
//! |---|---|---|
//! | `cycle.e64mib` | `cycle`, `floor.flush` | `cycle`: open (`Toy::open` with the production profile, as a new CLI process), one commit (`Toy::run`: phase 1's replay of the generation, the append, the data-only flush, the publish, P-88's two directory flushes), close (the drop: every handle closed, Defender's scan of the written files). `floor.flush`: one `write_at` of the drawn size and one `sync(Data)` on a raw 64 MiB extent |
//! | `close.e64mib` | `close` | the close of the handle that just committed: the Defender close cost of a written `log.1`, `HEAD` and `LOCK` (G8, U10) |
//!
//! **Setup and teardown stay outside every timed region** ([MP §4.1]): the store generation's retirement and `init`
//! ([`store::Generations`]), the draw of the sizes, the counters' first reading, the floor extent's creation, and for
//! `close` the open and the commit; after the second tick, the counters check and the counting of the commit. A sample
//! with a batch of k makes the setup of all k operations before its first tick ([MP §4.5]); every operation takes
//! milliseconds, so the batch is 1 in practice.
//!
//! **The counters checks** (on the real OS, every run and smoke): a `cycle` sample makes exactly one class-`durable`
//! flush per commit and no `durable+meta` flush (the phase-2b flush, `write.rs:1186`; a commit without rotation
//! prepares no extent, `write.rs:738-763`); its two directory flushes (P-88, `store.rs:529-544`) are reported by the
//! smoke, not asserted. A `floor.flush` sample makes exactly one `durable` flush and no other.
//!
//! **Sizes** (R-31 Q2): typical commits only, `moirai_toylog::commit_size(draw, 600)`, 300–600 B (`config.rs:112-113`);
//! the size tail is measurement 2's. The draws are XXH3-seeded by `m1:<quantity>` for the fillers and by
//! `m1-floor:<quantity>` for the floor ([`store::draw64`]). The floor's extent is E = 64 MiB, zero-filled at creation; a
//! run writes at most about 55,000 × 600 B ≈ 33 MB to it, so the new extent its arm makes when one is full is never
//! needed in a run, and when made it is made in setup.
//!
//! **Generations** ([`store::Generations`]): each operation commits to the current `store.<g>`, retired after 4,096
//! commits (P06) or 4 MiB of fillers (P07); at typical sizes the commit bound binds. Every file lies below the run's
//! directory `<cwd>/m1.<pid>/`, removed on every path out of the run ([MP §9.7] "The working directory").

pub mod store;

use crate::arm::{Arm, ArmError, Ticker};
use crate::units::Unit;
use moirai_vfs::SyncKind;
use std::path::{Path, PathBuf};
use store::{Generations, Rig, draw64, seed};

/// The measurement's row number.
pub const MEASUREMENT: u32 = 1;
/// The largest filler: typical commits only (R-31 Q2), `commit_size(draw, 600)`.
pub const FILLER_MAX: u32 = 600;
/// The floor extent's length, E = 64 MiB (P01, the production profile).
pub const FLOOR_LEN: u64 = 64 << 20;

/// A stream of filler sizes: `commit_size(draw64(seed, i), FILLER_MAX)` for i = 0, 1, ….
#[derive(Clone, Debug)]
pub struct Sizes {
    seed: u64,
    i: u64,
}

impl Sizes {
    /// The stream named `name` (`m1:<quantity>` or `m1-floor:<quantity>`).
    pub fn new(name: &str) -> Sizes {
        Sizes {
            seed: seed(name),
            i: 0,
        }
    }

    /// The next size.
    pub fn next_size(&mut self) -> u32 {
        let d = draw64(self.seed, self.i);
        self.i += 1;
        moirai_toylog::commit_size(d, FILLER_MAX)
    }
}

/// The flushes between two counter readings: (data, data+meta, directory).
fn flushes(c0: &moirai_vfs::VfsCounters, c1: &moirai_vfs::VfsCounters) -> (u64, u64, u64) {
    (
        c1.sync_data.saturating_sub(c0.sync_data),
        c1.sync_meta.saturating_sub(c0.sync_meta),
        c1.sync_dir.saturating_sub(c0.sync_dir),
    )
}

/// Arm 0 of `cycle.e64mib`: open, one commit, close.
pub struct CycleArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    gens: Generations,
    sizes: Sizes,
    next_op: u64,
    ticker: T,
    batch: u32,
    /// The directory flushes of the last sample (reported by the smoke).
    pub last_dir_flushes: u64,
}

impl<'r, R: Rig, T: Ticker> CycleArm<'r, R, T> {
    /// The arm over `rig`, committing to `gens` with fillers from `sizes`, timed by `ticker`.
    pub fn new(rig: &'r R, gens: Generations, sizes: Sizes, ticker: T) -> CycleArm<'r, R, T> {
        CycleArm {
            rig,
            gens,
            sizes,
            next_op: 1,
            ticker,
            batch: 1,
            last_dir_flushes: 0,
        }
    }
}

impl<R: Rig, T: Ticker> Arm for CycleArm<'_, R, T> {
    fn name(&self) -> &str {
        "cycle"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let (dir, _) = self.gens.ready(self.rig).map_err(ArmError)?;
        let fillers: Vec<u32> = (0..k).map(|_| self.sizes.next_size()).collect();
        let ops: Vec<u64> = (0..k).map(|j| self.next_op + j).collect();
        let c0 = self.rig.counters();
        let t0 = self.ticker.now_ns();
        for (&op, &filler) in ops.iter().zip(&fillers) {
            let mut h = self.rig.open(&dir, 0).map_err(ArmError)?;
            self.rig.commit(&mut h, op, filler).map_err(ArmError)?;
            self.rig.close(h);
        }
        let t1 = self.ticker.now_ns();
        let c1 = self.rig.counters();
        self.next_op += k;
        let (data, meta, dirs) = flushes(&c0, &c1);
        if data != k || meta != 0 {
            return Err(ArmError(format!(
                "cycle: {k} commits made {data} durable and {meta} durable+meta flushes; one durable flush each and no \
                 durable+meta flush expected (write.rs:1186)"
            )));
        }
        self.last_dir_flushes = dirs;
        self.gens
            .add(k, fillers.iter().map(|&f| u64::from(f)).sum());
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// Arm 0 of `close.e64mib`: the close of the handle that committed; its open and commit are setup.
pub struct CloseArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    gens: Generations,
    sizes: Sizes,
    next_op: u64,
    ticker: T,
    batch: u32,
}

impl<'r, R: Rig, T: Ticker> CloseArm<'r, R, T> {
    /// The arm over `rig`, committing to `gens` with fillers from `sizes`, timed by `ticker`.
    pub fn new(rig: &'r R, gens: Generations, sizes: Sizes, ticker: T) -> CloseArm<'r, R, T> {
        CloseArm {
            rig,
            gens,
            sizes,
            next_op: 1,
            ticker,
            batch: 1,
        }
    }
}

impl<R: Rig, T: Ticker> Arm for CloseArm<'_, R, T> {
    fn name(&self) -> &str {
        "close"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let (dir, _) = self.gens.ready(self.rig).map_err(ArmError)?;
        let mut handles = Vec::with_capacity(self.batch as usize);
        let mut bytes = 0;
        for _ in 0..k {
            let filler = self.sizes.next_size();
            let mut h = self.rig.open(&dir, 0).map_err(ArmError)?;
            self.rig
                .commit(&mut h, self.next_op, filler)
                .map_err(ArmError)?;
            self.next_op += 1;
            bytes += u64::from(filler);
            handles.push(h);
        }
        let t0 = self.ticker.now_ns();
        for h in handles {
            self.rig.close(h);
        }
        let t1 = self.ticker.now_ns();
        self.gens.add(k, bytes);
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// Arm 1 of `cycle.e64mib`: one write of the drawn size and one data-only flush on a raw extent
/// (`60-roadmap.md:788`).
pub struct FloorArm<'r, R: Rig, T: Ticker> {
    rig: &'r R,
    base: PathBuf,
    len: u64,
    made: u32,
    floor: Option<R::Floor>,
    off: u64,
    sizes: Sizes,
    buf: Vec<u8>,
    ticker: T,
    batch: u32,
}

impl<'r, R: Rig, T: Ticker> FloorArm<'r, R, T> {
    /// The arm over `rig`: its extents of `len` bytes are `<base>/floor.<n>/floor.1`, its sizes from `sizes`.
    pub fn new(rig: &'r R, base: &Path, len: u64, sizes: Sizes, ticker: T) -> FloorArm<'r, R, T> {
        FloorArm {
            rig,
            base: base.to_path_buf(),
            len,
            made: 0,
            floor: None,
            off: 0,
            sizes,
            buf: vec![0x5A; FILLER_MAX as usize],
            ticker,
            batch: 1,
        }
    }

    /// The offset of the next write.
    pub fn offset(&self) -> u64 {
        self.off
    }
}

impl<R: Rig, T: Ticker> Arm for FloorArm<'_, R, T> {
    fn name(&self) -> &str {
        "floor.flush"
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let k = u64::from(self.batch);
        let sizes: Vec<usize> = (0..k).map(|_| self.sizes.next_size() as usize).collect();
        let need: u64 = sizes.iter().map(|&s| s as u64).sum();
        if self.floor.is_none() || self.off + need > self.len {
            self.made += 1;
            let dir = self.base.join(format!("floor.{}", self.made));
            self.floor = Some(self.rig.floor_create(&dir, self.len).map_err(ArmError)?);
            self.off = 0;
        }
        let floor = self.floor.as_ref().expect("made above");
        let mut offs = Vec::with_capacity(sizes.len());
        let mut off = self.off;
        for &s in &sizes {
            offs.push(off);
            off += s as u64;
        }
        let c0 = self.rig.counters();
        let t0 = self.ticker.now_ns();
        for (&o, &s) in offs.iter().zip(&sizes) {
            self.rig.write(floor, o, &self.buf[..s]).map_err(ArmError)?;
            self.rig.sync(floor, SyncKind::Data).map_err(ArmError)?;
        }
        let t1 = self.ticker.now_ns();
        let c1 = self.rig.counters();
        self.off = off;
        let (data, meta, dirs) = flushes(&c0, &c1);
        if data != k || meta != 0 || dirs != 0 {
            return Err(ArmError(format!(
                "floor.flush: {k} writes made {data} durable, {meta} durable+meta and {dirs} directory flushes; one \
                 durable flush each and no other expected"
            )));
        }
        Ok(t1.saturating_sub(t0) / k)
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

#[cfg(test)]
mod tests {
    use super::store::fake::{Ev, FakeRig, TraceTicker};
    use super::store::{GEN_BYTES, GEN_COMMITS};
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/w/m1.7")
    }

    fn cycle(rig: &FakeRig, gens: Generations) -> CycleArm<'_, FakeRig, TraceTicker> {
        CycleArm::new(rig, gens, Sizes::new("m1:cycle.e64mib"), rig.ticker())
    }

    fn gens() -> Generations {
        Generations::new(&base(), GEN_COMMITS, GEN_BYTES)
    }

    /// The trace from the first tick on.
    fn from_tick(t: &[Ev]) -> (Vec<Ev>, Vec<Ev>) {
        let i = t.iter().position(|e| *e == Ev::Tick).expect("a tick");
        (t[..i].to_vec(), t[i..].to_vec())
    }

    /// `cycle` times exactly open, commit and close of the current generation: the sample is 1,000 + 20,000 + 300 ns,
    /// and the generation's `init` lies before the first tick.
    #[test]
    fn cycle_times_open_commit_and_close_and_nothing_else() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, gens());
        let store = base().join("store.1");
        let mut sizes = Sizes::new("m1:cycle.e64mib");
        for (i, id) in [(0u64, 1u64), (1, 2)] {
            rig.clear();
            assert_eq!(arm.sample(), Ok(21_300));
            let (setup, timed) = from_tick(&rig.trace());
            let want_setup = if i == 0 {
                vec![Ev::Init(store.clone())]
            } else {
                vec![]
            };
            assert_eq!(setup, want_setup);
            assert_eq!(
                timed,
                [
                    Ev::Tick,
                    Ev::Open {
                        id,
                        dir: store.clone(),
                        cost: 0
                    },
                    Ev::Commit {
                        id,
                        op: i + 1,
                        filler: sizes.next_size()
                    },
                    Ev::Close(id),
                    Ev::Tick,
                ]
            );
        }
        assert_eq!(rig.world.borrow().stores[&store].len(), 2);
    }

    /// `close` times the close of the handle that committed, and only it: the open and the commit lie before the
    /// first tick, and the sample is the close's 300 ns.
    #[test]
    fn close_times_the_close_of_the_handle_that_committed() {
        let rig = FakeRig::new();
        let mut arm = CloseArm::new(&rig, gens(), Sizes::new("m1:close.e64mib"), rig.ticker());
        assert_eq!(arm.sample(), Ok(300));
        let (setup, timed) = from_tick(&rig.trace());
        let store = base().join("store.1");
        assert_eq!(setup.len(), 3);
        assert_eq!(setup[0], Ev::Init(store.clone()));
        assert_eq!(
            setup[1],
            Ev::Open {
                id: 1,
                dir: store,
                cost: 0
            }
        );
        assert!(matches!(setup[2], Ev::Commit { id: 1, op: 1, .. }));
        assert_eq!(timed, [Ev::Tick, Ev::Close(1), Ev::Tick]);
        assert_eq!(arm.name(), "close");
    }

    /// `floor.flush` times one write of the drawn size and one data-only flush (750 ns); the offsets advance by the
    /// drawn sizes; the store is never touched; a full extent is replaced in setup, before the first tick.
    #[test]
    fn floor_flush_times_one_write_and_one_data_only_flush() {
        let rig = FakeRig::new();
        let mut arm = FloorArm::new(
            &rig,
            &base(),
            1_200,
            Sizes::new("m1-floor:cycle.e64mib"),
            rig.ticker(),
        );
        let mut sizes = Sizes::new("m1-floor:cycle.e64mib");
        let (f1, f2) = (base().join("floor.1"), base().join("floor.2"));
        let mut off = 0u64;
        let mut floor = f1.clone();
        for i in 0..4 {
            rig.clear();
            assert_eq!(arm.sample(), Ok(750), "sample {i}");
            let s = sizes.next_size();
            let (setup, timed) = from_tick(&rig.trace());
            if i == 0 {
                assert_eq!(setup, [Ev::FloorCreate(f1.clone(), 1_200)]);
            } else if off + u64::from(s) > 1_200 {
                floor = f2.clone();
                off = 0;
                assert_eq!(setup, [Ev::FloorCreate(f2.clone(), 1_200)], "sample {i}");
            } else {
                assert!(setup.is_empty(), "sample {i}: {setup:?}");
            }
            assert_eq!(
                timed,
                [
                    Ev::Tick,
                    Ev::Write(floor.clone(), off, s as usize),
                    Ev::Sync(floor.clone(), SyncKind::Data),
                    Ev::Tick,
                ],
                "sample {i}"
            );
            off += u64::from(s);
            assert_eq!(arm.offset(), off);
        }
        assert_eq!(floor, f2, "four sizes of 300-600 B overflow 1,200 B once");
        let w = rig.world.borrow();
        assert!(
            w.stores.is_empty() && w.handles.is_empty(),
            "no store touched"
        );
        assert_eq!(arm.name(), "floor.flush");
    }

    /// A `cycle` whose commit makes a `durable+meta` flush, or two `durable` flushes, fails the arm with the counts.
    #[test]
    fn a_cycle_that_is_not_one_data_only_flush_fails() {
        for (extra, says) in [
            ((0, 1), "1 durable and 1 durable+meta"),
            ((1, 0), "2 durable and 0"),
        ] {
            let rig = FakeRig::new();
            rig.world.borrow_mut().commit_extra = extra;
            let mut arm = cycle(&rig, gens());
            let e = arm.sample().unwrap_err();
            assert!(e.0.contains(says), "{e}");
        }
    }

    /// The fillers are `commit_size(draw64(seed("m1:<q>"), i), 600)`, each in [300, 600] (R-31 Q2), and the floor's
    /// sizes come from their own seed.
    #[test]
    fn fillers_and_floor_writes_follow_their_seeded_streams() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, gens());
        let mut floor = FloorArm::new(
            &rig,
            &base(),
            FLOOR_LEN,
            Sizes::new("m1-floor:cycle.e64mib"),
            rig.ticker(),
        );
        let s = seed("m1:cycle.e64mib");
        let f = seed("m1-floor:cycle.e64mib");
        assert_ne!(s, f);
        for i in 0..200u64 {
            arm.sample().unwrap();
            floor.sample().unwrap();
            let want = moirai_toylog::commit_size(draw64(s, i), 600);
            let want_floor = moirai_toylog::commit_size(draw64(f, i), 600) as usize;
            assert!((300..=600).contains(&want));
            let t = rig.trace();
            let filler = t.iter().rev().find_map(|e| match e {
                Ev::Commit { filler, .. } => Some(*filler),
                _ => None,
            });
            let len = t.iter().rev().find_map(|e| match e {
                Ev::Write(_, _, len) => Some(*len),
                _ => None,
            });
            assert_eq!((filler, len), (Some(want), Some(want_floor)), "draw {i}");
            rig.clear();
        }
    }

    /// A due generation is retired and the next one `init`ed before the sample's first tick: the sample excludes
    /// `init`'s 10^9 ns.
    #[test]
    fn a_new_generation_is_made_before_the_sample_that_needs_it() {
        let rig = FakeRig::new();
        let mut arm = cycle(&rig, Generations::new(&base(), 2, GEN_BYTES));
        assert_eq!(arm.sample(), Ok(21_300));
        assert_eq!(arm.sample(), Ok(21_300));
        rig.clear();
        assert_eq!(arm.sample(), Ok(21_300));
        let (setup, timed) = from_tick(&rig.trace());
        assert_eq!(
            setup,
            [
                Ev::Init(base().join("store.2")),
                Ev::Remove(base().join("store.1"))
            ]
        );
        assert!(matches!(&timed[1], Ev::Open { dir, .. } if *dir == base().join("store.2")));
        assert!(matches!(timed[2], Ev::Commit { op: 3, .. }));
    }
}
