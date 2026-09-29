//! `CountingAlloc` installed as the global allocator, as a probe root installs it ([OS/mem §6]).

#![cfg(windows)]

use moirai_os::{CountingAlloc, OsMeter};
use moirai_vfs::Meter;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn heap_counts_follow_allocations() {
    let m = OsMeter;
    let before = m
        .heap_counts()
        .expect("installed: the process has allocated");
    let v = vec![7u8; 1 << 20];
    let during = m.heap_counts().unwrap();
    assert!(during.live_bytes >= before.live_bytes + (1 << 20));
    assert!(during.high_water_bytes >= during.live_bytes);
    drop(std::hint::black_box(v));
    let after = m.heap_counts().unwrap();
    assert!(after.live_bytes + (1 << 20) <= during.live_bytes + 65_536);
    assert!(
        after.high_water_bytes >= during.live_bytes,
        "the high-water mark stays"
    );
    m.reset_heap_high_water();
    let reset = m.heap_counts().unwrap();
    assert!(reset.high_water_bytes < during.live_bytes);
    // realloc grows and shrinks the live count.
    let mut s: Vec<u8> = Vec::with_capacity(16);
    s.extend(std::iter::repeat_n(1u8, 1 << 18));
    assert!(m.heap_counts().unwrap().live_bytes >= reset.live_bytes + (1 << 18));
    s.truncate(8);
    s.shrink_to_fit();
    assert!(m.heap_counts().unwrap().live_bytes < reset.live_bytes + (1 << 18));
}
