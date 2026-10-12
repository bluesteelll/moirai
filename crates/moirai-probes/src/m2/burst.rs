//! Measurement 2's burst: what each child acknowledges ([`Ack`]), the burst it belongs to ([`Burst`]), the measures
//! of the module docs of [`crate::m2`], each a function of one burst, and the [`Coordinator`] that runs bursts over a
//! pool of [`Writers`] and checks each.

use super::child::{Go, Reply};
use super::cost::{self, Shape};
use super::locks::{Byte, LockEvent};
use crate::arm::ArmError;
use crate::m1::store::{Generations, Rig, draw64, seed};
use std::time::{Duration, Instant};

/// One child's acknowledgement of its commit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ack {
    /// The commit's operation.
    pub op: u64,
    /// When the child started, at or after T.
    pub t_start: u64,
    /// When `Toy::run` returned: the acknowledgement.
    pub t_ack: u64,
    /// The child's class-`durable` flushes over the commit.
    pub sync_data: u64,
    /// Its class-`durable+meta` flushes.
    pub sync_meta: u64,
    /// The bytes it read over the open and the commit (the replay of the generation, mostly).
    pub bytes_read: u64,
    /// Its phase-1 time, in ns (`Note::Phase1`: the replay and the decision).
    pub phase1_ns: u64,
    /// Its writer- and flush-byte events, in order.
    pub events: Vec<LockEvent>,
}

impl Ack {
    /// The first writer grant of the commit, (req, grant): its append's.
    pub fn first_writer_grant(&self) -> Option<(u64, u64)> {
        self.events.iter().find_map(|e| match *e {
            LockEvent::Grant {
                byte: Byte::Writer,
                req,
                grant,
            } => Some((req, grant)),
            _ => None,
        })
    }

    /// How long the append held the writer byte: from its first writer grant to the writer release that follows it.
    pub fn append_hold(&self) -> Option<u64> {
        let i = self.events.iter().position(|e| {
            matches!(
                e,
                LockEvent::Grant {
                    byte: Byte::Writer,
                    ..
                }
            )
        })?;
        let LockEvent::Grant { grant, .. } = self.events[i] else {
            return None;
        };
        self.events[i + 1..].iter().find_map(|e| match *e {
            LockEvent::Release {
                byte: Byte::Writer,
                t,
            } => Some(t.saturating_sub(grant)),
            _ => None,
        })
    }
}

/// One burst: its start time T and the 16 acknowledgements in the order the `go` lines were sent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Burst {
    /// T.
    pub t0: u64,
    /// The acknowledgements, in `go` order.
    pub acks: Vec<Ack>,
}

/// The last acknowledgement: max(`t_ack`) − T.
pub fn last_ack(b: &Burst) -> u64 {
    b.acks
        .iter()
        .map(|a| a.t_ack)
        .max()
        .unwrap_or(b.t0)
        .saturating_sub(b.t0)
}

/// The burst's log data flushes: the sum of the children's class-`durable` flushes (directory and `durable+meta`
/// flushes are not counted).
pub fn flushes(b: &Burst) -> u64 {
    b.acks.iter().map(|a| a.sync_data).sum()
}

/// Fairness: over the children's first writer grants, the pairs whose later request was granted first (strictly on
/// both sides; ties count nothing). 0 is arrival order; 16 children give at most 120.
pub fn overtakes(b: &Burst) -> u64 {
    let mut firsts: Vec<(u64, u64)> = b.acks.iter().filter_map(Ack::first_writer_grant).collect();
    firsts.sort_unstable();
    let mut n = 0;
    for (j, &(req_j, grant_j)) in firsts.iter().enumerate() {
        n += firsts[..j]
            .iter()
            .filter(|&&(req_i, grant_i)| req_i < req_j && grant_j < grant_i)
            .count() as u64;
    }
    n
}

/// The flush-byte hand-offs: for each flush grant g, the latest flush release r of the burst (any child) with
/// r ≤ grant(g); when req(g) ≤ r the waiter was waiting at that release and grant(g) − r is one hand-off, otherwise g
/// is no hand-off. In `go` order, then event order.
pub fn handoffs(b: &Burst) -> Vec<u64> {
    let releases: Vec<u64> = b
        .acks
        .iter()
        .flat_map(|a| &a.events)
        .filter_map(|e| match *e {
            LockEvent::Release {
                byte: Byte::Flush,
                t,
            } => Some(t),
            _ => None,
        })
        .collect();
    b.acks
        .iter()
        .flat_map(|a| &a.events)
        .filter_map(|e| match *e {
            LockEvent::Grant {
                byte: Byte::Flush,
                req,
                grant,
            } => {
                let r = releases.iter().copied().filter(|&r| r <= grant).max()?;
                (req <= r).then(|| grant - r)
            }
            _ => None,
        })
        .collect()
}

/// The writer waits: grant − req of every writer grant of the burst, in `go` order, then event order.
pub fn writer_waits(b: &Burst) -> Vec<u64> {
    b.acks
        .iter()
        .flat_map(|a| &a.events)
        .filter_map(|e| match *e {
            LockEvent::Grant {
                byte: Byte::Writer,
                req,
                grant,
            } => Some(grant.saturating_sub(req)),
            _ => None,
        })
        .collect()
}

/// The child writers of a burst: 16 in a run.
pub const CHILDREN: usize = 16;
/// The lead between stamping T and T: the `go` lines are sent within it.
pub const LEAD_NS: u64 = 5_000_000;
/// The latest a child may start after T (runtime check 2).
pub const LATE_NS: u64 = 1_000_000;
/// How long a burst waits for its replies.
pub const BURST_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest filler: P05 `store.commit.inline-max-bytes`, 1 MiB, the full spec-derived size distribution.
pub const FILLER_MAX: u32 = 1 << 20;

/// The pool of child writers, as the coordinator sees it.
pub trait Writers {
    /// The number of children.
    fn count(&self) -> usize;
    /// Sends `go` to child `i`.
    fn send(&mut self, i: usize, go: &Go) -> Result<(), String>;
    /// The next reply of any child, waiting at most `timeout`.
    fn recv(&mut self, timeout: Duration) -> Result<(usize, Reply), String>;
}

/// The seed of a quantity's cost draws.
pub fn cost_seed(quantity: &str) -> u64 {
    seed(&format!("m2-cost:{quantity}"))
}

/// The seed of a quantity's filler sizes.
pub fn filler_seed(quantity: &str) -> u64 {
    seed(&format!("m2-filler:{quantity}"))
}

/// What a run's bursts report beside their measures, for the results analyst: per ack the bytes it read and its
/// phase-1 time (the replay's share, module docs "Generations and the replay"), per burst the latest start after T.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// The bytes each commit read.
    pub bytes_read: Vec<u64>,
    /// Each commit's phase-1 time, in ns.
    pub phase1_ns: Vec<u64>,
    /// Each burst's latest start after T, in ns.
    pub start_lag: Vec<u64>,
    /// The generations made.
    pub generations: u32,
}

impl Report {
    /// One line: p50, p99 and maximum of each series (nearest rank), and the counts.
    pub fn summary(&self) -> String {
        fn stats(v: &[u64]) -> String {
            let mut s = v.to_vec();
            s.sort_unstable();
            let at = |p: f64| -> u64 {
                let i = (p * s.len() as f64).ceil() as usize;
                s.get(i.max(1) - 1).copied().unwrap_or(0)
            };
            format!(
                "p50 {} p99 {} max {}",
                at(0.5),
                at(0.99),
                s.last().copied().unwrap_or(0)
            )
        }
        format!(
            "replay: bytes read per commit {}; phase 1 ns {}; latest start after T ns {}; {} commits, {} bursts, {} \
             generations",
            stats(&self.bytes_read),
            stats(&self.phase1_ns),
            stats(&self.start_lag),
            self.bytes_read.len(),
            self.start_lag.len(),
            self.generations
        )
    }
}

/// The coordinator of a run's bursts (module docs of [`crate::m2`], "The burst"): it owns the generations and the
/// reader, draws each burst's costs and fillers, sends the `go` lines, collects the replies and makes the runtime
/// checks.
pub struct Coordinator<'a, R: Rig> {
    rig: &'a R,
    writers: &'a mut dyn Writers,
    gens: Generations,
    reader: Option<R::Reader>,
    shape: Shape,
    scale: u32,
    cost_seed: u64,
    filler_seed: u64,
    bursts: u64,
    /// The run's report.
    pub report: Report,
}

impl<'a, R: Rig> Coordinator<'a, R> {
    /// The coordinator of quantity `quantity`'s bursts over `writers`, with the cost `shape` at `scale` %.
    pub fn new(
        rig: &'a R,
        writers: &'a mut dyn Writers,
        gens: Generations,
        quantity: &str,
        shape: Shape,
        scale: u32,
    ) -> Coordinator<'a, R> {
        Coordinator {
            rig,
            writers,
            gens,
            reader: None,
            shape,
            scale,
            cost_seed: cost_seed(quantity),
            filler_seed: filler_seed(quantity),
            bursts: 0,
            report: Report::default(),
        }
    }

    /// One burst and its runtime checks; the acks of the burst come back in `go` order.
    pub fn burst(&mut self) -> Result<Burst, ArmError> {
        let n = self.writers.count();
        let k = self.bursts;
        // Setup: the generation (its reader closed before a retired one is removed), the reader, the draws.
        if self.gens.due() {
            self.reader = None;
        }
        let (dir, made) = self.gens.ready(self.rig).map_err(ArmError)?;
        self.report.generations += u32::from(made);
        if self.reader.is_none() {
            self.reader = Some(self.rig.reader(&dir).map_err(ArmError)?);
        }
        let first = (k % n as u64) as usize;
        let mut gos: Vec<(usize, Go)> = (0..n)
            .map(|j| {
                let i = (first + j) % n;
                let index = k * n as u64 + i as u64;
                let go = Go {
                    op: index + 1,
                    start_ns: 0,
                    cost_ns: cost::draw(self.cost_seed, index, self.shape, self.scale),
                    filler: moirai_toylog::commit_size(draw64(self.filler_seed, index), FILLER_MAX),
                    dir: dir.clone(),
                };
                (i, go)
            })
            .collect();
        // The burst: T, the go lines, the replies.
        let t0 = self.rig.mono_ns() + LEAD_NS;
        for (i, go) in &mut gos {
            go.start_ns = t0;
            self.writers.send(*i, go).map_err(ArmError)?;
        }
        self.bursts += 1;
        let deadline = Instant::now() + BURST_TIMEOUT;
        let mut acks: Vec<Option<Ack>> = vec![None; n];
        let mut got = 0;
        while got < n {
            let left = deadline.saturating_duration_since(Instant::now());
            let (i, reply) = self
                .writers
                .recv(left)
                .map_err(|m| ArmError(format!("burst {k}: {got} of {n} acks: {m}")))?;
            match reply {
                Reply::Ack(a) if i < n && acks[i].is_none() => {
                    acks[i] = Some(a);
                    got += 1;
                }
                Reply::Ack(_) => {
                    return Err(ArmError(format!(
                        "burst {k}: a second reply from child {i}"
                    )));
                }
                Reply::Err { op, message } => {
                    return Err(ArmError(format!(
                        "burst {k}, child {i}, op {op}: {message}"
                    )));
                }
            }
        }
        // Teardown: the store as a reader sees it, then the checks, in go order.
        let seen = self
            .rig
            .commits(self.reader.as_mut().expect("opened above"))
            .map_err(ArmError)?;
        let mut ordered = Vec::with_capacity(n);
        let mut fillers = 0;
        for (i, go) in &gos {
            let a = acks[*i].take().expect("every child replied");
            let fail =
                |what: String| ArmError(format!("burst {k}, child {i}, op {}: {what}", go.op));
            if a.op != go.op {
                return Err(fail(format!("the ack is for op {}", a.op)));
            }
            if a.t_start < t0 || a.t_start - t0 > LATE_NS {
                return Err(fail(format!(
                    "it started at T{:+} ns, outside [T, T + {LATE_NS} ns]",
                    i128::from(a.t_start) - i128::from(t0)
                )));
            }
            if !a
                .first_writer_grant()
                .is_some_and(|(req, _)| req >= a.t_start)
            {
                return Err(fail("no writer grant requested after its start".into()));
            }
            let hold = a.append_hold().unwrap_or(0);
            if hold < go.cost_ns || a.t_ack.saturating_sub(a.t_start) < go.cost_ns {
                return Err(fail(format!(
                    "its append held the writer byte {hold} ns, below its in-lock cost of {} ns",
                    go.cost_ns
                )));
            }
            if !seen.contains(&go.op) {
                return Err(fail("Toy::read does not see its commit".into()));
            }
            fillers += u64::from(go.filler);
            ordered.push(a);
        }
        let expected = self.gens.commits() + n as u64;
        if seen.len() as u64 != expected {
            return Err(ArmError(format!(
                "burst {k}: Toy::read sees {} commits in the generation, {expected} expected",
                seen.len()
            )));
        }
        let b = Burst { t0, acks: ordered };
        if flushes(&b) == 0 {
            return Err(ArmError(format!("burst {k}: no log data flush")));
        }
        self.gens.add(n as u64, fillers);
        for a in &b.acks {
            self.report.bytes_read.push(a.bytes_read);
            self.report.phase1_ns.push(a.phase1_ns);
        }
        let lag = b.acks.iter().map(|a| a.t_start - t0).max().unwrap_or(0);
        self.report.start_lag.push(lag);
        Ok(b)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::m1::store::draw64;

    pub(crate) fn wg(req: u64, grant: u64) -> LockEvent {
        LockEvent::Grant {
            byte: Byte::Writer,
            req,
            grant,
        }
    }

    pub(crate) fn wr(t: u64) -> LockEvent {
        LockEvent::Release {
            byte: Byte::Writer,
            t,
        }
    }

    fn fg(req: u64, grant: u64) -> LockEvent {
        LockEvent::Grant {
            byte: Byte::Flush,
            req,
            grant,
        }
    }

    fn fr(t: u64) -> LockEvent {
        LockEvent::Release {
            byte: Byte::Flush,
            t,
        }
    }

    fn ack(events: Vec<LockEvent>) -> Ack {
        Ack {
            events,
            ..Ack::default()
        }
    }

    fn burst(acks: Vec<Ack>) -> Burst {
        Burst { t0: 1_000, acks }
    }

    /// The naive count: every pair of children, by index, compared on their first writer grants.
    fn naive_overtakes(pairs: &[(u64, u64)]) -> u64 {
        let mut n = 0;
        for i in 0..pairs.len() {
            for j in 0..pairs.len() {
                if pairs[i].0 < pairs[j].0 && pairs[j].1 < pairs[i].1 {
                    n += 1;
                }
            }
        }
        n
    }

    /// On 10,000 seeded scenarios of 16 (request, grant) pairs, with ties, `overtakes` equals the naive pairwise count.
    #[test]
    fn overtakes_equal_the_naive_count() {
        let mut nonzero = 0;
        for s in 0..10_000u64 {
            let pairs: Vec<(u64, u64)> = (0..16u64)
                .map(|i| {
                    let d = draw64(s, i);
                    let req = d % 64;
                    (req, req + (d >> 32) % 512)
                })
                .collect();
            let b = burst(
                pairs
                    .iter()
                    .map(|&(r, g)| ack(vec![wg(r, g), wr(g + 1)]))
                    .collect(),
            );
            let n = overtakes(&b);
            assert_eq!(n, naive_overtakes(&pairs), "scenario {s}: {pairs:?}");
            nonzero += u64::from(n > 0);
        }
        assert!(nonzero > 9_000, "the scenarios overtake");
    }

    /// Only each commit's first writer grant counts (a flush holder's later scan and publish grants never do); FIFO
    /// gives 0, the reverse 120, equal requests 0.
    #[test]
    fn overtakes_count_only_the_first_writer_grant_of_each_commit() {
        let fifo = burst(
            (0..16)
                .map(|i| ack(vec![wg(i, 100 + i), wr(101 + i)]))
                .collect(),
        );
        assert_eq!(overtakes(&fifo), 0);
        let reverse = burst(
            (0..16)
                .map(|i| ack(vec![wg(i, 200 - i), wr(201 - i)]))
                .collect(),
        );
        assert_eq!(overtakes(&reverse), 120);
        let equal = burst((0..16).map(|i| ack(vec![wg(5, 100 + i)])).collect());
        assert_eq!(overtakes(&equal), 0);
        // Child 0 asks first and is granted first; its later scan grant comes after child 1's: not an overtake.
        let later = burst(vec![
            ack(vec![
                wg(0, 10),
                wr(11),
                fg(12, 12),
                wg(13, 30),
                wr(31),
                fr(32),
            ]),
            ack(vec![wg(1, 20), wr(21)]),
        ]);
        assert_eq!(overtakes(&later), 0);
        assert_eq!(later.acks[0].first_writer_grant(), Some((0, 10)));
        assert_eq!(later.acks[0].append_hold(), Some(1));
    }

    /// A flush grant pairs with the latest flush release at or before it, from any child: a waiter that asked before
    /// that release is one hand-off; an uncontended grant (asked after it) is none, and so is a grant with no release
    /// before it.
    #[test]
    fn handoffs_pair_each_waiting_grant_with_the_release_before_it() {
        let b = burst(vec![
            ack(vec![fg(100, 100), fr(300)]),
            ack(vec![fg(150, 320), fr(400), fg(500, 500), fr(510)]),
            ack(vec![fg(200, 450), fr(460)]),
        ]);
        // Child 0's grant has no release before it; child 1's first waited from 150 for the release at 300 (20 ns);
        // its second asked at 500 after the last release (460): none; child 2's waited from 200: the latest release
        // before 450 is child 1's at 400 (50 ns), not child 0's at 300.
        assert_eq!(handoffs(&b), [20, 50]);
    }

    /// The writer waits are grant − req of every writer grant, in `go` order then event order; flush grants are never
    /// taken.
    #[test]
    fn writer_waits_are_every_writer_grant_in_writer_order() {
        let b = burst(vec![
            ack(vec![
                wg(10, 15),
                wr(20),
                fg(21, 40),
                wg(41, 47),
                wr(48),
                fr(49),
            ]),
            ack(vec![fg(1, 2), wg(12, 22), wr(23)]),
        ]);
        assert_eq!(writer_waits(&b), [5, 6, 10]);
    }

    /// The flushes are the sum of the children's data flushes; `durable+meta` flushes are never added.
    #[test]
    fn flushes_sum_the_data_flushes_of_the_burst() {
        let mut acks: Vec<Ack> = (0..16)
            .map(|i| Ack {
                sync_data: u64::from(i % 3 == 0),
                sync_meta: 2,
                ..Ack::default()
            })
            .collect();
        assert_eq!(flushes(&burst(acks.clone())), 6);
        acks[1].sync_data = 2;
        assert_eq!(flushes(&burst(acks)), 8);
    }

    /// The last acknowledgement is the latest `t_ack` less T: not the earliest, and not from a start time.
    #[test]
    fn last_ack_is_the_latest_ack_after_the_start_time() {
        let b = Burst {
            t0: 1_000,
            acks: vec![
                Ack {
                    t_start: 1_200,
                    t_ack: 5_000,
                    ..Ack::default()
                },
                Ack {
                    t_start: 1_100,
                    t_ack: 9_000,
                    ..Ack::default()
                },
                Ack {
                    t_start: 1_050,
                    t_ack: 3_000,
                    ..Ack::default()
                },
            ],
        };
        assert_eq!(last_ack(&b), 8_000);
        assert_eq!(last_ack(&Burst::default()), 0);
    }

    use crate::m1::store::fake::{Ev, FakeRig};
    use crate::m1::store::{GEN_BYTES, GEN_COMMITS};
    use crate::m2::child::fake::FakeWriters;
    use std::path::{Path, PathBuf};

    fn run_dir() -> PathBuf {
        PathBuf::from("/w/m2.1")
    }

    /// A coordinator over 16 fake children for quantity `q` at `shape` and `scale`, with generations of
    /// `max_commits`; runs `f` on it.
    fn with_coordinator<T>(
        rig: &FakeRig,
        pool: &mut FakeWriters,
        max_commits: u64,
        q: &str,
        shape: Shape,
        scale: u32,
        f: impl FnOnce(&mut Coordinator<'_, FakeRig>) -> T,
    ) -> T {
        let gens = Generations::new(&run_dir(), max_commits, GEN_BYTES);
        let mut c = Coordinator::new(rig, pool, gens, q, shape, scale);
        f(&mut c)
    }

    /// The coordinator's own events: setup, T, the go lines, the replies and the store read.
    fn coordinator_events(t: &[Ev]) -> Vec<Ev> {
        t.iter()
            .filter(|e| {
                matches!(
                    e,
                    Ev::Init(_)
                        | Ev::Remove(_)
                        | Ev::ReaderOpen(_)
                        | Ev::Read(_)
                        | Ev::Stamp(_)
                        | Ev::Go { .. }
                        | Ev::Ack { .. }
                )
            })
            .cloned()
            .collect()
    }

    /// A burst gives 16 acks, one per child, and the reader's `Toy::read` sees exactly the 16 new commits; the next
    /// burst's 16 follow.
    #[test]
    fn a_burst_yields_sixteen_acks_and_the_store_sees_sixteen_commits() {
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        with_coordinator(
            &rig,
            &mut pool,
            GEN_COMMITS,
            "last-ack.c0",
            Shape::Design,
            0,
            |c| {
                let b = c.burst().unwrap();
                assert_eq!(b.acks.len(), 16);
                let ops: Vec<u64> = b.acks.iter().map(|a| a.op).collect();
                assert_eq!(ops, (1..=16).collect::<Vec<u64>>());
                let store = rig.world.borrow().stores[&run_dir().join("store.1")].clone();
                assert_eq!(store, (1..=16).collect());
                let b = c.burst().unwrap();
                assert_eq!(b.acks.len(), 16);
                assert_eq!(
                    rig.world.borrow().stores[&run_dir().join("store.1")].len(),
                    32
                );
                assert_eq!(c.report.start_lag, [0, 0]);
                assert_eq!(c.report.bytes_read.len(), 32);
            },
        );
    }

    /// A lost commit, an ack for another op, a child that never replies and an ack without a writer grant each fail
    /// the burst, naming the child.
    #[test]
    fn a_lost_commit_or_a_wrong_ack_fails_the_burst() {
        type Script = fn(&FakeRig, &mut FakeWriters);
        let cases: [(Script, &str); 4] = [
            (
                |rig, _| {
                    rig.world.borrow_mut().lost_ops.insert(5);
                },
                "child 4, op 5: Toy::read does not see its commit",
            ),
            (
                |_, pool| {
                    pool.misreport.insert(3, 99);
                },
                "child 2, op 3: the ack is for op 99",
            ),
            (|_, pool| pool.silent.push(7), "15 of 16 acks"),
            (
                |rig, _| {
                    rig.world.borrow_mut().quiet_ops.insert(12);
                },
                "child 11, op 12: no writer grant",
            ),
        ];
        for (script, says) in cases {
            let rig = FakeRig::new();
            let mut pool = FakeWriters::new(&rig, CHILDREN);
            script(&rig, &mut pool);
            let e = with_coordinator(
                &rig,
                &mut pool,
                GEN_COMMITS,
                "last-ack.c0",
                Shape::Design,
                0,
                |c| c.burst().unwrap_err(),
            );
            assert!(e.0.contains(says), "{says}: {e}");
        }
    }

    /// Child i's commit of burst k carries the cost `cost::draw(cost_seed(q), 16k + i, shape, scale)` in the `Config`
    /// it opens with, and the filler `commit_size(draw64(filler_seed(q), 16k + i), 1 MiB)`, for a `Design` and a
    /// `Uniform` quantity.
    #[test]
    fn costs_and_fillers_follow_the_seeded_draws() {
        for (q, shape) in [
            ("last-ack.c100", Shape::Design),
            ("last-ack.a100", Shape::Uniform),
        ] {
            let rig = FakeRig::new();
            let mut pool = FakeWriters::new(&rig, CHILDREN);
            with_coordinator(&rig, &mut pool, GEN_COMMITS, q, shape, 100, |c| {
                c.burst().unwrap();
                c.burst().unwrap();
            });
            let t = rig.trace();
            let mut costs = std::collections::BTreeMap::new();
            let mut seen = 0;
            for e in &t {
                match e {
                    Ev::Open { id, cost, .. } => {
                        costs.insert(*id, *cost);
                    }
                    Ev::Commit { id, op, filler } => {
                        let index = op - 1;
                        let want = cost::draw(cost_seed(q), index, shape, 100);
                        assert_eq!(costs[id], want, "{q} op {op}");
                        let size =
                            moirai_toylog::commit_size(draw64(filler_seed(q), index), 1 << 20);
                        assert_eq!(*filler, size, "{q} op {op}");
                        seen += 1;
                    }
                    _ => {}
                }
            }
            assert_eq!(seen, 32);
            assert!(
                costs.values().any(|&c| c > 1_000_000),
                "{q}: the draws reach the ms range"
            );
            assert_ne!(cost_seed(q), filler_seed(q));
        }
    }

    /// The generation's `init`, the reader's opening and its read lie outside the burst: T is stamped after setup,
    /// then the 16 go lines and the 16 replies, then the store is read; the last acknowledgement is the children's
    /// serialised open + commit (1,000 + 16 × 20,000 ns), even in the burst that retires a generation (init's 10^9 ns
    /// stay out).
    #[test]
    fn setup_and_the_store_read_stay_outside_the_burst() {
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        let store = |g: u32| run_dir().join(format!("store.{g}"));
        with_coordinator(&rig, &mut pool, 16, "last-ack.c0", Shape::Design, 0, |c| {
            for g in [1u32, 2] {
                rig.clear();
                let b = c.burst().unwrap();
                assert_eq!(last_ack(&b), 1_000 + 16 * 20_000, "generation {g}");
                let ev = coordinator_events(&rig.trace());
                let mut want = vec![Ev::Init(store(g))];
                if g == 2 {
                    want.push(Ev::Remove(store(1)));
                }
                want.push(Ev::ReaderOpen(store(g)));
                assert_eq!(ev[..want.len()], want[..], "generation {g}");
                let rest = &ev[want.len()..];
                assert!(matches!(rest[0], Ev::Stamp(t) if t + LEAD_NS == b.t0));
                assert!(rest[1..17].iter().all(|e| matches!(e, Ev::Go { .. })));
                assert!(rest[17..33].iter().all(|e| matches!(e, Ev::Ack { .. })));
                assert_eq!(rest[33..], [Ev::Read(store(g))]);
            }
        });
    }

    /// A due generation retires between bursts: the next burst commits to `store.<g+1>`, the old one's reader is
    /// closed and the old directory removed in that burst's setup, before T.
    #[test]
    fn generations_retire_between_bursts() {
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        with_coordinator(&rig, &mut pool, 32, "flushes.c0", Shape::Design, 0, |c| {
            c.burst().unwrap();
            c.burst().unwrap();
            rig.clear();
            c.burst().unwrap();
            assert_eq!(c.report.generations, 2);
        });
        let t = rig.trace();
        let at = |e: &Ev| t.iter().position(|x| x == e).unwrap();
        let stamp = t.iter().position(|e| matches!(e, Ev::Stamp(_))).unwrap();
        assert!(at(&Ev::Init(run_dir().join("store.2"))) < stamp);
        assert!(
            at(&Ev::Remove(run_dir().join("store.1")))
                < at(&Ev::ReaderOpen(run_dir().join("store.2")))
        );
        assert!(at(&Ev::ReaderOpen(run_dir().join("store.2"))) < stamp);
        assert!(
            t.iter().all(|e| !matches!(e, Ev::Open { dir, .. } if dir.as_path() != Path::new("/w/m2.1/store.2"))),
            "every commit of the third burst goes to store.2"
        );
        assert!(!rig.world.borrow().live.contains(&run_dir().join("store.1")));
    }

    /// A commit whose append did not hold the writer byte for its cost (its `Config` lost the draw) fails the burst,
    /// even when its acknowledgement came later than the cost; with the cost spent the same burst passes.
    #[test]
    fn a_commit_that_did_not_spend_its_cost_in_the_append_fails_the_burst() {
        for ignore in [true, false] {
            let rig = FakeRig::new();
            {
                let mut w = rig.world.borrow_mut();
                w.lat.open = 25_000_000;
                w.ignore_cost = ignore;
            }
            let mut pool = FakeWriters::new(&rig, CHILDREN);
            let r = with_coordinator(
                &rig,
                &mut pool,
                GEN_COMMITS,
                "last-ack.c100",
                Shape::Design,
                100,
                |c| c.burst(),
            );
            if ignore {
                let e = r.unwrap_err();
                assert!(
                    e.0.contains(
                        "its append held the writer byte 20000 ns, below its in-lock cost"
                    ),
                    "{e}"
                );
            } else {
                assert!(r.is_ok(), "{r:?}");
            }
        }
    }

    /// A child that starts more than 1 ms after T fails the burst; one that starts 1 ms after T is within the bound,
    /// and the report records the lag.
    #[test]
    fn a_late_start_fails_the_burst() {
        for (late, ok) in [(LATE_NS + 1, false), (LATE_NS, true)] {
            let rig = FakeRig::new();
            let mut pool = FakeWriters::new(&rig, CHILDREN);
            pool.late.insert(3, late);
            let (r, lags) = with_coordinator(
                &rig,
                &mut pool,
                GEN_COMMITS,
                "last-ack.c0",
                Shape::Design,
                0,
                |c| (c.burst(), c.report.start_lag.clone()),
            );
            if ok {
                assert!(r.is_ok(), "{r:?}");
                assert_eq!(lags, [LATE_NS]);
            } else {
                let e = r.unwrap_err();
                assert!(
                    e.0.contains("child 2, op 3: it started at T+1000001 ns"),
                    "{e}"
                );
            }
        }
    }

    /// Burst k sends its go lines from child k mod 16 round, so no child is always first; its acks come back in
    /// that order.
    #[test]
    fn the_go_order_rotates_by_burst() {
        let rig = FakeRig::new();
        let mut pool = FakeWriters::new(&rig, CHILDREN);
        with_coordinator(
            &rig,
            &mut pool,
            GEN_COMMITS,
            "handoff.c0",
            Shape::Design,
            0,
            |c| {
                for k in 0..3u64 {
                    rig.clear();
                    let b = c.burst().unwrap();
                    let gos: Vec<usize> = rig
                        .trace()
                        .iter()
                        .filter_map(|e| match e {
                            Ev::Go { child, .. } => Some(*child),
                            _ => None,
                        })
                        .collect();
                    let want: Vec<usize> = (0..16).map(|j| (k as usize + j) % 16).collect();
                    assert_eq!(gos, want, "burst {k}");
                    let ops: Vec<u64> = b.acks.iter().map(|a| a.op).collect();
                    let want_ops: Vec<u64> = want.iter().map(|&i| 16 * k + i as u64 + 1).collect();
                    assert_eq!(ops, want_ops, "burst {k}");
                }
            },
        );
    }
}
