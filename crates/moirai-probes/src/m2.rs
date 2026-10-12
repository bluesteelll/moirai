//! Measurement 2 (WP-52, [60 §5.2] item 2, `60-roadmap.md:758`): the group-commit protocol under contention. 16 writer
//! processes each make one CLI commit per burst through the toy log on the real `Vfs` — the writer and flush bytes on
//! the overlapped lock (`LockFileEx`) — with an in-lock cost drawn per commit from the hold budget (R-24 Q2, R-31 Q1).
//! It decides the M1 leader: out only if the last acknowledgement's p99 stays ≤ 50 ms at the maximum in-lock cost
//! (`60-roadmap.md:758`, `ARCHITECTURE-RESEARCH.md:1947`; a 25 ms guard on the idle run), judged at tier t2 (R-24 Q9).
//! Its writer wait is T2's value for measurement 14, which builds no second driver (R-24 Q7).
//!
//! **The burst.** The driver process is the coordinator; it keeps a pool of 16 child writers (`m2 --writer`, the same
//! executable) for the whole run. Burst k:
//! 1. Setup, untimed: retire the store generation when it is due (the coordinator's reader of the old one is closed
//!    first), open the reader of the current one, and draw child i's cost (`cost::draw`, index 16k + i, the quantity's
//!    shape and scale) and filler (`commit_size(draw, 1 MiB)`, the full spec-derived distribution, which R-31 Q2 leaves
//!    to this measurement), each stream XXH3-seeded by the quantity; child i commits op 16k + i + 1.
//! 2. T = now + 5 ms (the lead).
//! 3. `go` to every child, in an order that starts at child k mod 16 and rotates by one each burst, so no child is
//!    systematically the first to hear its `go` and the values a block discards fall on every child alike.
//! 4. Each child sleeps until 2 ms before T and polls to T, stamps `t_start`, makes its CLI commit — `Toy::open` with a
//!    new `Config` carrying its draw (`Config::in_lock_cost_ns`) and a new `ProcLocks`, as a new CLI process does, then
//!    `Toy::run` — stamps `t_ack` when `run` returns (the acknowledgement), reads its counters, lock events and phase-1
//!    time, closes the handle and replies `ack`; nothing of it is open when the coordinator reads.
//! 5. The coordinator collects the 16 replies (60 s at most), then teardown, untimed: `Toy::read` on its reader.
//!
//! **The runtime checks of every burst** (every run and smoke; a failed check fails the arm, so the run fails):
//! 1. 16 acks, each for its own op.
//! 2. Every start lies in [T, T + 1 ms]. A late `go` would add its lateness to the last acknowledgement, so a burst
//!    with a late start is refused rather than measured: the lead's dispatch adds less than 1 ms to any value.
//! 3. Each child's append held the writer byte at least its cost: from its first writer grant to the release that
//!    follows it. The cost is spent inside that holding (`write.rs:188-200`, called at `:843`), so a commit whose
//!    `Config` lost its draw fails here as soon as a draw exceeds the append's own hold. `t_ack − t_start ≥ cost` too.
//! 4. Each ack carries a writer grant requested at or after its `t_start`: its append.
//! 5. `Toy::read` sees the 16 ops, and the generation grew by exactly 16 commits.
//! 6. The burst made at least one log data flush.
//!
//! **The measures** (per burst; every time on the system-wide monotonic clock, comparable across the processes of one
//! boot, `docs/spec/os/clock.md:42`):
//!
//! | Measure (quantity prefix) | Arm, unit | Definition |
//! |---|---|---|
//! | Last acknowledgement (`last-ack`) | `last-ack`, ns | max(`t_ack`) − T |
//! | Flushes per burst (`flushes`) | `flushes`, count | the sum of the 16 children's class-`durable` flushes: the log data flushes (`moirai-vfs/src/fs.rs:283-284`; AR:1797 "≤ 3 per 16-writer burst"); P-88's 32 directory flushes are not counted |
//! | Fairness (`fairness`) | `overtakes`, count | over each child's first writer grant of the commit (its append's), the pairs (i, j) with req_i < req_j and grant_j < grant_i: requests overtaken by a later request. 0 is arrival order, which [80 §2.2.1] item 10 says Windows roughly keeps; at most 120; ties count nothing |
//! | Flush-byte hand-off (`handoff`) | `handoff`, ns | for each flush grant g, the latest flush release r of the burst (any child) with r ≤ grant(g); when req(g) ≤ r the waiter was waiting at the release, and grant(g) − r is one hand-off; otherwise none |
//! | Writer wait (`writer-wait`, T2) | `writer-wait`, ns | grant − req of every writer grant of the burst: every acquisition `lock.writer-wait-ms` bounds, the flush holder's scan and publish re-acquisitions included (`write.rs:1073`, `:1123`) |
//!
//! `last-ack`, `flushes` and `overtakes` take one value per burst; `writer-wait` and `handoff` take every value of a
//! burst, in the burst's `go` order and then event order. An arm runs bursts until it holds its batch of values ([MP
//! §4.5]); a new block discards what an earlier block left. 100 bursts in a row without a value fail the arm.
//!
//! **The cost** (R-31 Q1 (C), `cost`): a draw per commit, `round(scale / 100 × Q(u))`, from one of two shapes, each a
//! table of anchors. `Design` (the decision's): linear from 0 to 50 µs below the median ("appends of tens of µs under
//! the writer byte", AR:1717), log-linear to 5 ms at p99, linear to 20 ms at the maximum (the hold budget, AR:1798).
//! `Uniform` (the sensitivity run): uniform from 0 to 5 ms below p99, linear to 20 ms: the maximum-entropy body. A
//! quantity is `<measure>.<shape><scale>`: shape `c` is `Design`, `a` is `Uniform`, scale NN is NN % of the draw, a
//! sweep from 0 to the hold budget (`60-roadmap.md:758`), never a constant (R-24 Q2). The decision follows `Design`;
//! `last-ack.a100` shows how far it depends on the unmeasured body. When the two disagree on "leader in", that is an
//! owner question at WP-81a (R-31).
//!
//! **Generations and the replay** (the critique's I-1). A child's commit opens a fresh handle, whose phase 1 replays the
//! generation's log from its start (`store.rs:1124-1180`), outside the writer byte but between T and `t_ack`: it is part
//! of the last acknowledgement, as a product open's replay of its tail is. A generation retires at 4,096 commits (P06)
//! or 4 MiB of fillers (P07), the design figures (`17-store-parameters.md:171-172`); a CLI-only store checkpoints at
//! P13 = 2 times them (`:329`), so the bound is the lower reading. With this measurement's sizes (a mean filler of about
//! 14 KB, about 230 KB a burst) a generation lasts about 18 bursts, and an open replays from 0 to about 4 MiB plus the
//! burst's earlier commits. The toy reads every filler byte at replay, where the product may load bodies lazily, so the
//! replay's share is reported for the results analyst: each ack carries the child's bytes read and its phase-1 time
//! (`Note::Phase1`), and the driver prints their p50, p99 and maximum after every run, into the attempt's log.
//!
//! **Pool and setup.** The arm and its coordinator are built per run; the pool is started once per process and is
//! reused by the smoke's quantities. Every file lies below the run's directory `<cwd>/m2.<pid>/`, removed on every path
//! out of the run ([MP §9.7]); a retired generation is removed in setup.

pub mod burst;
pub mod child;
pub mod cost;
pub mod locks;
