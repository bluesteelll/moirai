//! The simulated random source ([OS/README §4.6] "Simulator form"; [F15 §6.4] "Determinism", §6.3 A-7): one seeded
//! stream per simulated process, every draw in the trace, scripted values for the draw-again branches.

mod common;

use common::*;
use moirai_vfs::Entropy;
use moirai_vfs_sim::{EventKind, SimConfig, SimWorld, TraceMode};

fn draws(seed: u64) -> (Vec<[u8; 16]>, Vec<u8>) {
    let w = SimWorld::new(SimConfig::new(seed));
    let a = w.process("a");
    let b = w.process("b");
    let mut out = Vec::new();
    for v in [&a, &b, &a, &b] {
        let mut x = [0u8; 16];
        v.fill_random(&mut x);
        out.push(x);
    }
    (out, w.trace_bytes())
}

#[test]
fn every_process_has_its_own_replayable_stream() {
    let (x, tx) = draws(90);
    let (y, ty) = draws(90);
    assert_eq!(x, y, "a seed replays its draws");
    assert_eq!(tx, ty, "and its trace");
    let (z, _) = draws(91);
    assert_ne!(x, z, "another seed draws other values");
    // Two processes never share a stream, and a stream does not repeat.
    let distinct: std::collections::BTreeSet<_> = x.iter().collect();
    assert_eq!(distinct.len(), 4);
    // A process's stream does not depend on the other processes' draws.
    let w = SimWorld::new(SimConfig::new(90));
    let a = w.process("a");
    let _b = w.process("b");
    let mut first = [0u8; 16];
    let mut second = [0u8; 16];
    a.fill_random(&mut first);
    a.fill_random(&mut second);
    assert_eq!([first, second], [x[0], x[2]]);
}

#[test]
fn draws_are_traced_and_scripted_values_come_first() {
    let w = world(92);
    let (v, _) = proc(&w, "p");
    // A test scripts an all-zero value and a repeat, to reach a caller's draw-again branch.
    let mut seen = [0u8; 8];
    v.fill_random(&mut seen);
    let mut script = vec![0u8; 8];
    script.extend_from_slice(&seen);
    w.script_random(&v, &script);
    let mut a = [9u8; 8];
    let mut b = [9u8; 8];
    let mut c = [9u8; 12];
    v.fill_random(&mut a);
    v.fill_random(&mut b);
    v.fill_random(&mut c);
    assert_eq!(a, [0u8; 8]);
    assert_eq!(b, seen);
    assert_ne!(c, [9u8; 12]);
    let ev: Vec<(u64, u64)> = w
        .trace()
        .iter()
        .filter(|e| e.kind == EventKind::Random && e.proc == v.process())
        .map(|e| (e.a, e.c))
        .collect();
    assert_eq!(ev, vec![(8, 0), (8, 8), (8, 8), (12, 0)]);
    // The digest-only trace proves the same draws.
    let mut cfg = SimConfig::new(92);
    cfg.trace = TraceMode::DigestOnly;
    let lean = SimWorld::new(cfg);
    let mut full_cfg = SimConfig::new(92);
    full_cfg.trace = TraceMode::Full;
    let full = SimWorld::new(full_cfg);
    for w in [&lean, &full] {
        let p = w.process_with("p", None, Some(true));
        let mut x = [0u8; 32];
        p.fill_random(&mut x);
    }
    assert_eq!(lean.trace_digest(), full.trace_digest());
}
