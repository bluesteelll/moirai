//! Performance through the public API.
//!
//! The tier-`pr` tests check deterministic work and memory on large inputs and print their durations: PR CI is never a
//! timing gate (PLAN WP-04). With `MOIRAI_TEST_TIER` set to `nightly` or `exit` they also assert a generous time bound.
//! The complexity counters of the histogram diff are checked by its unit tests.
//!
//! `worst_case_timings` is the measurement [F12 §7.5] "Complexity" asks WP-60 for: HD and diff3 on worst-case texts at
//! the 65,536-byte bound of a text value ([F08 §5.3]) and on usual edits. It is ignored by default; run it optimised:
//!
//! ```text
//! cargo test -p moirai-diff --release --test perf -- --ignored --nocapture worst_case_timings
//! ```

use std::time::{Duration, Instant};

use moirai_diff::{Differ, Hit, Interner, Match, Pattern, Selector, diff_lines, levenshtein};

/// Generous enough for a loaded machine running an unoptimised build; asserted only in the `nightly` and `exit` tiers.
const LIMIT: Duration = Duration::from_secs(30);

/// Prints a duration, and asserts [`LIMIT`] outside tier `pr`.
fn timed(what: &str, elapsed: Duration) {
    println!("{what}: {elapsed:?}");
    let tier = std::env::var("MOIRAI_TEST_TIER").unwrap_or_default();
    if matches!(tier.as_str(), "nightly" | "exit") {
        assert!(elapsed < LIMIT, "{what}: {elapsed:?}");
    }
}

/// A deterministic xorshift generator.
fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut x = seed | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    }
}

/// Source-like text: lines of words from a small vocabulary, deterministic in `seed`.
fn source_text(seed: u64, lines: usize) -> Vec<u8> {
    const WORDS: [&str; 16] = [
        "let", "mut", "fn", "self", "value", "return", "match", "Some", "None", "=>", "{", "}",
        "(", ")", "index", "len",
    ];
    let mut next = rng(seed);
    let mut out = Vec::new();
    for n in 0..lines {
        let words = 2 + (next() % 9) as usize;
        for w in 0..words {
            if w > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(WORDS[(next() % 16) as usize].as_bytes());
        }
        // A line number keeps most lines distinct, as in real code.
        out.extend_from_slice(format!(" // {n}\n").as_bytes());
    }
    out
}

#[test]
fn fuzzy_quote_search_over_four_mib() {
    let text = source_text(11, 125_000);
    assert!(text.len() >= 4 << 20, "{}", text.len());
    // A 128-byte quote taken from the middle, then edited in three places: k = ⌊128 / 4⌋ = 32 ([F20 §6.4] draft).
    let at = text.len() / 2;
    let mut quote = text[at..at + 128].to_vec();
    quote[10] = b'#';
    quote.remove(70);
    quote.insert(100, b'@');
    let p = Pattern::new(&quote);
    assert!(p.heap_bytes() <= 8 << 10, "{}", p.heap_bytes());
    let k = quote.len() / 4;
    let start = Instant::now();
    let mut count = 0usize;
    let mut candidates: Vec<Hit> = Vec::new();
    // Streamed through a 64 KiB buffer, as the resolver reads a file; the selector keeps the candidates.
    let mut s = p.searcher(k);
    let mut sel = Selector::new(quote.len(), k);
    let mut held = 0;
    for chunk in text.chunks(64 << 10) {
        s.feed(chunk, |h| {
            count += 1;
            sel.push(h, |c| candidates.push(c));
        });
        sel.advance(s.position(), |c| candidates.push(c));
        held = held.max(sel.held());
    }
    sel.finish(|c| candidates.push(c));
    timed("fuzzy quote search over 4 MiB", start.elapsed());
    assert!(count < 1000, "{count}");
    assert!(
        held <= sel.lag() + 1 + 33 * (sel.lag() / quote.len() + 2),
        "{held}"
    );
    // One candidate: the edited quote.
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    let best = candidates[0];
    assert!(best.distance <= 3, "{best:?}");
    assert!(best.end.abs_diff(at + 128) <= 3, "{best:?}");
    let located = p
        .locate(&text, best.end, 0, best.distance)
        .expect("a hit has a start");
    assert!(located.start.abs_diff(at) <= 3, "{located:?}");
    assert_eq!(
        levenshtein(&quote, &text[located.start..best.end]),
        best.distance
    );
}

#[test]
fn line_diff_of_large_texts() {
    let old = source_text(21, 200_000);
    // Edit every 997th line and move one block of 500 lines to the end.
    let lines: Vec<&[u8]> = moirai_diff::lines(&old).collect();
    let mut new: Vec<u8> = Vec::with_capacity(old.len() + 4096);
    for (n, line) in lines.iter().enumerate() {
        if (50_000..50_500).contains(&n) {
            continue;
        }
        if n % 997 == 0 {
            new.extend_from_slice(b"changed line\n");
        } else {
            new.extend_from_slice(line);
        }
    }
    for line in &lines[50_000..50_500] {
        new.extend_from_slice(line);
    }
    let start = Instant::now();
    let m = diff_lines(&old, &new).unwrap();
    timed("line diff of 200,000 lines", start.elapsed());
    assert!(m.windows(2).all(|w| w[0].p < w[1].p && w[0].q < w[1].q));
    // Every unedited line outside the moved block stays matched.
    assert!(m.len() >= 200_000 - 500 - 200_000 / 997 - 1, "{}", m.len());

    // The same through a reused differ over interned ids: scratch stays linear in the input.
    let mut int = Interner::new();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    int.intern_lines(&old, &mut a).unwrap();
    int.intern_lines(&new, &mut b).unwrap();
    let mut d = Differ::new();
    let mut again = Vec::new();
    d.diff(&a, &b, &mut again).unwrap();
    assert_eq!(again, m);
    assert!(
        d.scratch_bytes() <= 48 * (a.len() + b.len()) + (1 << 20),
        "{}",
        d.scratch_bytes()
    );
}

#[test]
fn line_diff_with_every_other_line_changed() {
    // 40,000 lines with about two copies of each, and every other line replaced: each split of HD peels one region,
    // and a diff that rescans the rest of the box at each split does O(n²) work (seconds even when optimised).
    let n = 40_000u32;
    let mut next = rng(5);
    let p: Vec<u32> = (0..n).map(|_| (next() % u64::from(n / 2)) as u32).collect();
    let q: Vec<u32> = p
        .iter()
        .enumerate()
        .map(|(k, &v)| if k % 2 == 1 { n + k as u32 } else { v })
        .collect();
    let mut d = Differ::new();
    let mut m = Vec::new();
    let start = Instant::now();
    d.diff(&p, &q, &mut m).unwrap();
    d.diff(&q, &p, &mut m).unwrap();
    timed("two diffs with every other line changed", start.elapsed());
    assert!(m.windows(2).all(|w| w[0].p < w[1].p && w[0].q < w[1].q));
    assert!(
        d.scratch_bytes() <= 48 * 2 * n as usize + (1 << 20),
        "{}",
        d.scratch_bytes()
    );
}

/// The 65,536-byte bound of a `text` value and of a body ([F08 §5.3]).
const TEXT_MAX: usize = 65_536;

/// The `k`-th of 8,100 distinct 3-byte lines: two printable bytes and LF.
fn line3(k: usize) -> [u8; 3] {
    [b'!' + (k / 90 % 90) as u8, b'!' + (k % 90) as u8, b'\n']
}

/// Joins lines into a text.
fn join<L: AsRef<[u8]>>(lines: impl IntoIterator<Item = L>) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(l.as_ref());
    }
    out
}

/// The lines of `text`, reversed.
fn reversed(text: &[u8]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = moirai_diff::lines(text).collect();
    lines.reverse();
    join(lines)
}

/// The lines of `text` in an order shuffled by `seed` (Fisher–Yates on xorshift).
fn shuffled(text: &[u8], seed: u64) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = moirai_diff::lines(text).collect();
    let mut next = rng(seed);
    for k in (1..lines.len()).rev() {
        lines.swap(k, (next() % (k as u64 + 1)) as usize);
    }
    join(lines)
}

/// `text` with every `every`-th line replaced by a 3-byte line that occurs nowhere else (at most 6,100 of them).
fn sprinkled(text: &[u8], every: usize) -> Vec<u8> {
    join(moirai_diff::lines(text).enumerate().map(|(n, l)| {
        if n % every == every / 2 {
            line3(2000 + n / every).to_vec()
        } else {
            l.to_vec()
        }
    }))
}

/// 64 copies of a block of 300 distinct lines, each followed by the separator line `sep + copy` (57,792 bytes). With
/// other separators on the other side, every step of HD matches one copy and lowers the counts of the 300 ids, whose
/// regions are the same runs.
fn shapes_block(sep: usize) -> Vec<u8> {
    join((0..64).flat_map(|c| (0..300).map(line3).chain([line3(sep + c)])))
}

/// A worst-case pair (P, Q) of texts at the byte bound, and its description.
struct Shape {
    name: &'static str,
    p: Vec<u8>,
    q: Vec<u8>,
}

/// The texts [`worst_case_timings`] measures, each at most [`TEXT_MAX`] bytes.
fn shapes() -> Vec<Shape> {
    // 341 distinct 3-byte lines in a cycle repeated 64 times: 21,824 lines, 65,472 bytes, every count 64.
    let cycle = join((0..64).flat_map(|_| (0..341).map(line3)));
    // A copy of v (and of v w) at every one of 64 places in P, and Q made only of them: 64 × |Q| rare pairs.
    let junk = |k: usize| line3(1000 + k);
    let rare_p = join((0..64).flat_map(|k| [b"v\n".to_vec(), junk(k).to_vec()]));
    let rare_q = b"v\n".repeat(TEXT_MAX / 2);
    let pair_p = join((0..64).flat_map(|k| [b"v\n".to_vec(), b"w\n".to_vec(), junk(k).to_vec()]));
    let pair_q = b"v\nw\n".repeat(TEXT_MAX / 4);
    // Lines drawn at random from 341: counts near 64 on both sides of the limit.
    let mut next = rng(77);
    let random = join((0..TEXT_MAX / 3).map(|_| line3((next() % 341) as usize)));
    let mut next = rng(78);
    let random2 = join((0..TEXT_MAX / 3).map(|_| line3((next() % 341) as usize)));
    // A source file of about 2,000 lines under a usual edit: a few changed lines and a moved block.
    let mut source = source_text(3, 2_300);
    source.truncate(TEXT_MAX);
    let cut = source[..TEXT_MAX / 2]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |at| at + 1);
    source.truncate(
        source
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |at| at + 1),
    );
    let edited = sprinkled(&source, 150);
    let block_end = cut + 2_000;
    let block_end = source[block_end..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |at| block_end + at + 1);
    let mut moved = source[..cut].to_vec();
    moved.extend_from_slice(&source[block_end..]);
    moved.extend_from_slice(&source[cut..block_end]);
    vec![
        Shape {
            name: "cycle of 341 lines x 64 vs its reversal",
            p: cycle.clone(),
            q: reversed(&cycle),
        },
        Shape {
            name: "cycle of 341 lines x 64 vs a shuffle",
            p: cycle.clone(),
            q: shuffled(&cycle, 5),
        },
        Shape {
            name: "64 x (v, unique) vs v x 32,768",
            p: rare_p,
            q: rare_q,
        },
        Shape {
            name: "64 x (v, w, unique) vs (v, w) x 16,384",
            p: pair_p,
            q: pair_q,
        },
        Shape {
            name: "64 copies of a 300-line block, separators differ",
            p: shapes_block(7000),
            q: shapes_block(7100),
        },
        Shape {
            name: "21,845 random lines of 341 vs another",
            p: random.clone(),
            q: random2,
        },
        Shape {
            name: "21,845 random lines of 341, every 7th changed",
            p: random.clone(),
            q: sprinkled(&random, 7),
        },
        Shape {
            name: "source, every 150th line changed",
            p: source.clone(),
            q: edited,
        },
        Shape {
            name: "source, a 2,000-byte block moved",
            p: source,
            q: moved,
        },
    ]
}

/// HD of two texts' lines through a fresh differ: the pairs, the time of the diff alone and the scratch it held.
fn hd(p: &[u8], q: &[u8]) -> (Vec<Match>, Duration, usize) {
    let mut int = Interner::new();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    int.intern_lines(p, &mut a).unwrap();
    int.intern_lines(q, &mut b).unwrap();
    let mut d = Differ::new();
    let mut m = Vec::new();
    let start = Instant::now();
    d.diff(&a, &b, &mut m).unwrap();
    let elapsed = start.elapsed();
    assert!(m.windows(2).all(|w| w[0].p < w[1].p && w[0].q < w[1].q));
    assert!(m.iter().all(|x| a[x.p as usize] == b[x.q as usize]));
    (m, elapsed, d.scratch_bytes())
}

/// The least of three runs of `f`, which returns a duration.
fn best_of_3(mut f: impl FnMut() -> Duration) -> Duration {
    (0..3).map(|_| f()).min().unwrap_or_default()
}

#[test]
#[ignore = "a measurement: run optimised with --ignored --nocapture"]
fn worst_case_timings() {
    let shapes = shapes();
    println!("HD at the 65,536-byte bound (best of 3; lines of P, lines of Q; pairs; scratch):");
    for s in &shapes {
        assert!(s.p.len() <= TEXT_MAX && s.q.len() <= TEXT_MAX, "{}", s.name);
        let (lp, lq) = (
            moirai_diff::lines(&s.p).count(),
            moirai_diff::lines(&s.q).count(),
        );
        let mut info = (0, 0);
        let fwd = best_of_3(|| {
            let (m, t, bytes) = hd(&s.p, &s.q);
            info = (m.len(), bytes);
            t
        });
        let back = best_of_3(|| hd(&s.q, &s.p).1);
        println!(
            "  {:<48} {lp:>6} {lq:>6}  HD(P,Q) {fwd:>10.2?}  HD(Q,P) {back:>10.2?}  pairs {:>6}  scratch {:>8} B",
            s.name, info.0, info.1
        );
    }
    // diff3(O, A, B) runs HD(O, A) and HD(O, B): the worst pairs of shapes on one base, and a usual three-way edit.
    println!("diff3 (the two HDs, best of 3):");
    let by_name = |n: &str| shapes.iter().find(|s| s.name.starts_with(n)).unwrap();
    let cycle = by_name("cycle of 341 lines x 64 vs its reversal");
    let shuffle = by_name("cycle of 341 lines x 64 vs a shuffle");
    let source = by_name("source, every 150th line changed");
    let moved = by_name("source, a 2,000-byte block moved");
    let rare = by_name("64 x (v, unique)");
    let pairs = by_name("64 x (v, w, unique)");
    let blocks = by_name("64 copies of a 300-line block");
    let third_blocks = shapes_block(7200);
    for (name, o, a, b) in [
        (
            "cycle: reversal and shuffle",
            &cycle.p,
            &cycle.q,
            &shuffle.q,
        ),
        (
            "v x 64: floods of v and of (v, w)",
            &rare.p,
            &rare.q,
            &pairs.q,
        ),
        (
            "blocks: two other separators",
            &blocks.p,
            &blocks.q,
            &third_blocks,
        ),
        (
            "source: edits and a moved block",
            &source.p,
            &source.q,
            &moved.q,
        ),
    ] {
        let t = best_of_3(|| hd(o, a).1 + hd(o, b).1);
        println!("  {name:<48} {t:>10.2?}");
    }
}
