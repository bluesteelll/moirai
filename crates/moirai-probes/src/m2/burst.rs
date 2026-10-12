//! Measurement 2's burst: what each child acknowledges ([`Ack`]), the burst it belongs to ([`Burst`]), and the measures
//! of the module docs of [`crate::m2`], each a function of one burst.

use super::locks::{Byte, LockEvent};

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
}
