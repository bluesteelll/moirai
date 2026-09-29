//! The model at scale ([PLAN] WP-70's ≈ 2,000-node fixture store, WP-91's and WP-94's ≤ 512 MB per case): the cost of
//! one write grows with the store linearly, not quadratically — the time per write is compared at two store sizes —
//! and the `state_at` cache stays bounded while the history grows, whatever the number of lanes. The runtime bounds
//! are generous ratios (a debug build on a loaded runner); the memory bound is structural — the number of whole states
//! the model keeps — since the model reads no process counter (WP-94 runs every case under `probes peak`).

use super::*;
use crate::dag::{Dag, MAX_CHECKPOINTS, RECENT};
use std::time::{Duration, Instant};

/// Blocks `from..to` of 50 tasks each, each block a chain of `blocks` edges under one parent (each block its own
/// command, so none replays an earlier one).
fn grow(s: &mut S, from: usize, to: usize) {
    for b in from..to {
        let mut stmts = vec![task("p", &format!("parent {b}"))];
        for i in 0..49 {
            let mut c = child(
                &format!("c{i}"),
                &format!("task {b}.{i}"),
                Target::Var("p".into()),
            );
            if i > 0
                && let Stmt::Create { edges_in, .. } = &mut c
            {
                edges_in.push(("blocks".into(), Target::Var(format!("c{}", i - 1))));
            }
            stmts.push(c);
        }
        s.ok(tx(stmts), orch());
    }
}

/// The time per write of `rounds` rounds of 30 writes each on the store's tasks from `from` on — a task blocked by an
/// earlier one, a title change and a status move, in turn — the least round's mean, which a loaded runner disturbs
/// least.
fn per_write(s: &mut S, from: u32, rounds: u32) -> Duration {
    let mut best = Duration::MAX;
    for r in 0..rounds {
        let start = Instant::now();
        for j in 0..30u32 {
            let i = from + r * 30 + j;
            let reply = match i % 3 {
                0 => s.ok(
                    tx(vec![Stmt::Create {
                        name: Some("x".into()),
                        kind: "task".into(),
                        fields: vec![("title".into(), t(&format!("late {i}")))],
                        body: None,
                        under: None,
                        position: None,
                        edges_out: vec![],
                        edges_in: vec![("blocks".into(), Target::Id(Nid(2 + i % 150)))],
                    }]),
                    orch(),
                ),
                1 => s.ok(
                    tx(vec![set(3 + i, &[("title", t(&format!("renamed {i}")))])]),
                    orch(),
                ),
                _ => s.ok(
                    tx(vec![set(4 + i, &[("status", t("in_progress"))])]),
                    orch(),
                ),
            };
            assert!(reply.rev_new.is_some());
        }
        best = best.min(start.elapsed() / 30);
    }
    best
}

#[test]
fn writes_stay_linear_and_the_cache_bounded() {
    let mut s = S::base();
    grow(&mut s, 0, 4);
    assert_eq!(s.st.next_id, 201);
    let small = per_write(&mut s, 0, 3);
    grow(&mut s, 4, 32);
    assert_eq!(s.st.next_id, 201 + 30 + 1400);
    let large = per_write(&mut s, 200, 3);
    // Eight times the nodes: a write whose cost is linear in the store takes about eight times as long, a quadratic
    // one sixty-four times; the bound leaves a factor of three for noise.
    assert!(
        large.as_secs_f64() <= small.as_secs_f64() * 24.0,
        "{large:?} per write at 1,630 nodes against {small:?} at 200"
    );
    let bound = s.st.dag.live_refs().count() + MAX_CHECKPOINTS + RECENT;
    assert!(
        s.st.dag.cached_states() <= bound,
        "{} states cached, bound {bound}",
        s.st.dag.cached_states()
    );
    // An old state is re-folded on demand and equals the fold from the root.
    let st = s.st.dag.state_at(Some(5), &s.st.alloc);
    assert_eq!(*st, s.st.dag.state_from_scratch(Some(5), &s.st.alloc));
}

/// A deterministic SplitMix64 stream for the measured history.
struct Mix(u64);

impl Mix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

struct Fixed;

impl crate::state::Alloc for Fixed {
    fn uid(&self, n: Nid) -> crate::value::Uid {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&n.0.to_be_bytes());
        crate::value::Uid(b)
    }

    fn creator(&self, _: Nid) -> crate::state::Creator {
        crate::state::Creator {
            actor: "orchestrator".into(),
            role: "orchestrator".into(),
        }
    }
}

/// What a measured history found.
struct Measured {
    /// The bytes of one whole state.
    one_state: usize,
    /// The greatest bytes of the cache, measured after every fifth commit.
    peak: usize,
    /// The bytes of the commits with their changesets at the end.
    commits: usize,
    /// The most whole states the cache held after any commit.
    most_states: usize,
    /// The most checkpoint states it held after any commit.
    most_checkpoints: usize,
    /// The commits at a checkpoint depth of the final interval over every chain: what a cache that kept each chain's
    /// checkpoints would hold.
    checkpoint_depths: usize,
}

/// A history over `nodes` tasks (10 % Cyrillic titles, bodies of a few hundred bytes): commit 1 creates them on
/// `main`, then `commits` commits spread over `main` and `lanes` lanes that each merge `main` in every 40 of their
/// commits, every commit's candidate state remembered as the Store remembers it, a read of a random older commit after
/// every fourth commit (every hundredth read checked against the fold from the root), and the cache measured after
/// every commit. The history's refs are the test's own, so the cache keeps no tip: what it holds is its checkpoints
/// and recent states.
fn measured_history(nodes: u32, commits: u64, lanes: usize) -> Measured {
    use crate::dag::Commit;
    use crate::heap::state_bytes;
    use crate::state::{Aspect, Changeset, KState, KVal, Key, Node, State, diff};
    use crate::value::Value;
    use std::rc::Rc;

    let mut rng = Mix(91);
    let text = |rng: &mut Mix, len: usize| -> String {
        let cyr = rng.below(10) == 0;
        (0..len)
            .map(|i| match (cyr, i % 7) {
                (_, 6) => ' ',
                (true, _) => char::from_u32(0x0430 + rng.below(32) as u32).unwrap_or('а'),
                (false, _) => char::from(b'a' + rng.below(26) as u8),
            })
            .collect()
    };
    let mut dag = Dag::default();
    let mut st0 = State::default();
    for n in 1..=nodes {
        let id = Nid(n);
        let mut x = Node::new(
            crate::state::Alloc::uid(&Fixed, id),
            "task",
            &st0.schema,
            crate::state::Alloc::creator(&Fixed, id),
        );
        let title = text(&mut rng, 40);
        x.set_field(&st0.schema, "title", Some(Value::Text(title)));
        let len = 200 + rng.below(400) as usize;
        x.body = Some(text(&mut rng, len));
        st0.nodes.insert(id, x);
    }
    let one_state = state_bytes(&st0);
    let mut c1 = Commit::new(
        1,
        0,
        1,
        "ordinary",
        vec![],
        1,
        diff(&State::default(), &st0),
    );
    c1.append_hlc = 1;
    dag.insert(c1);
    dag.remember(1, Rc::new(st0));
    // Tips: index 0 is `main`, then the lanes; each lane forks at commit 1.
    let mut tips: Vec<u64> = vec![1; lanes + 1];
    let mut since_merge = vec![0u32; lanes + 1];
    let (mut peak, mut most_states, mut most_checkpoints) = (0usize, 0usize, 0usize);
    let mut reads = 0u64;
    for seq in 2..=commits {
        let b = rng.below(lanes as u64 + 1) as usize;
        let p1 = tips[b];
        let parent = dag.state_at(Some(p1), &Fixed);
        let mut st = (*parent).clone();
        let (parents, cs): (Vec<u64>, Changeset) = if b > 0 && since_merge[b] >= 40 {
            // A merge of `main` into the lane: main's nodes where the lane's differ, the net changeset its state diff.
            since_merge[b] = 0;
            let m = dag.state_at(Some(tips[0]), &Fixed);
            for (id, x) in &m.nodes {
                if st.nodes.get(id) != Some(x) && rng.below(2) == 0 {
                    st.nodes.insert(*id, x.clone());
                }
            }
            (vec![p1, tips[0]], diff(&parent, &st))
        } else {
            since_merge[b] += 1;
            let mut cs = Changeset::new();
            for _ in 0..1 + rng.below(5) {
                let id = Nid(1 + rng.below(u64::from(nodes)) as u32);
                let x = st.nodes.get_mut(&id).expect("a node");
                let before = x.fields.get("title").cloned();
                let len = 20 + rng.below(40) as usize;
                let after = Value::Text(text(&mut rng, len));
                x.set_field(&parent.schema, "title", Some(after.clone()));
                let key = Key::Node(id, Aspect::Field("title".into()));
                let old = cs
                    .remove(&key)
                    .map_or(KState::Plain(before.map(KVal::Value)), |(o, _)| o);
                cs.insert(key, (old, KState::Plain(Some(KVal::Value(after)))));
            }
            (vec![p1], cs)
        };
        drop(parent);
        let mut c = Commit::new(seq, b as u32, seq, "ordinary", parents, seq, cs);
        c.append_hlc = seq;
        dag.insert(c);
        dag.remember(seq, Rc::new(st));
        tips[b] = seq;
        if seq.is_multiple_of(4) {
            let old = 1 + rng.below(seq);
            let got = dag.state_at(Some(old), &Fixed);
            reads += 1;
            if reads.is_multiple_of(100) {
                assert_eq!(*got, dag.state_from_scratch(Some(old), &Fixed), "s{old}");
            }
        }
        if seq.is_multiple_of(5) {
            peak = peak.max(dag.cache_bytes());
        }
        most_states = most_states.max(dag.cached_states());
        most_checkpoints = most_checkpoints.max(dag.checkpoint_states());
    }
    Measured {
        one_state,
        peak,
        commits: dag.commit_bytes(),
        most_states,
        most_checkpoints,
        checkpoint_depths: dag.checkpoint_depth_commits(),
    }
}

/// WP-91's measurement of the `state_at` cache ([PLAN] WP-91: "memoised within ≤ 512 MB per case"): a history over the
/// ≈ 2,000-node fixture store of WP-70, 1,200 commits over `main` and five lanes. The bytes of the cache at their peak
/// and of the commits with their changesets at the end ([`crate::heap`]) stay within 512 MiB, and the cache holds a
/// bounded number of whole states: at most [`MAX_CHECKPOINTS`] + [`RECENT`], whatever the number of lanes.
#[test]
fn the_state_cache_measures_within_512_mib() {
    let m = measured_history(2_000, 1_200, 5);
    assert!(
        m.peak + m.commits <= 512 << 20,
        "{} bytes",
        m.peak + m.commits
    );
    assert!(
        m.most_states <= MAX_CHECKPOINTS + RECENT,
        "{} states",
        m.most_states
    );
    assert!(
        m.peak <= m.one_state * 4 * (MAX_CHECKPOINTS + RECENT),
        "{} bytes at the peak, one state {} bytes",
        m.peak,
        m.one_state
    );
}

/// The checkpoint part of the cache does not grow with the number of lanes: over twenty lanes whose chains each reach
/// several checkpoint depths — more such commits in all than [`MAX_CHECKPOINTS`] — the cache keeps at most
/// [`MAX_CHECKPOINTS`] checkpoints and [`RECENT`] recent states.
#[test]
fn the_checkpoints_stay_bounded_over_twenty_lanes() {
    let m = measured_history(100, 4_000, 20);
    assert!(
        m.checkpoint_depths > MAX_CHECKPOINTS,
        "the history reaches {} checkpoint depths over its chains",
        m.checkpoint_depths
    );
    assert!(
        m.most_checkpoints <= MAX_CHECKPOINTS,
        "{} checkpoints",
        m.most_checkpoints
    );
    assert!(
        m.most_states <= MAX_CHECKPOINTS + RECENT,
        "{} states",
        m.most_states
    );
}
