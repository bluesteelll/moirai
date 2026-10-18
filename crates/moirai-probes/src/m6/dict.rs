//! Measurement 6's raw-content dictionary (cut D-2, R-33 Q1 (b)): the probe's own deterministic trainer.
//!
//! It is the epoch cover of Liao, Petri, Moffat and Wirth (WWW 2016), the method `ruzstd`'s `dict_builder` names,
//! with exact d-mer counts in place of that builder's unseeded reservoir sample, so every process trains the same
//! bytes and option (3)'s proxy uses "the same raw-content dictionary" (`90-harness-agnostic-design.md:968`). A
//! product trainer may do better or worse, so the gain this dictionary shows is this trainer's.
//!
//! - **The sample** `S`: the training inputs in id order, up to the last whole input that fits in P16 = 4 MiB
//!   ([F17 §3]), concatenated. When `|S|` is at most [`DICT_MAX`], the dictionary is `S`.
//! - **The parameters**: d-mers of [`D`] = 8 bytes (`ruzstd`'s cover uses `K` = 16) and segments of [`SEGMENT`] =
//!   2,048 bytes (`ruzstd`'s `segment_size`), so [`EPOCHS`] = 32 epochs fill 65,536 bytes.
//! - **Epoch `e`** is `S[e·L/32 .. (e+1)·L/32)` with integer division; its candidates are its non-overlapping
//!   2,048-byte chunks from its start, a shorter last chunk included (`ruzstd`'s `.chunks(segment_size)`).
//! - **Counts**: every d-byte window of `S` (step 1) is counted once over the whole of `S`. A chunk's score is the sum
//!   of the current counts of its distinct d-mers (the windows that lie inside it).
//! - **The choice**: epoch by epoch, the highest-scoring chunk, ties to the earliest; then the counts of its d-mers
//!   are set to 0.
//! - **The output**: the 32 chosen chunks in ascending order of their score at choice time, ties by position, so the
//!   best lies nearest the input, as in `ruzstd`. At most [`DICT_MAX`] bytes ([F10 §6.2]).
//!
//! The counts live in a hash map that is only looked up, never iterated, and every tie is broken by position, so no
//! map order can decide the output.

use super::corpus::Input;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The largest dictionary: `dict.D`'s raw content is at most 65,536 bytes ([F10 §6.2]).
pub const DICT_MAX: usize = 65_536;
/// P16, the training sample's cap ([F17 §3], [F17 §6.3]).
pub const P16: usize = 4 * 1_024 * 1_024;
/// The d-mer length.
pub const D: usize = 8;
/// The candidate segment's length.
pub const SEGMENT: usize = 2_048;
/// The epochs: one chosen segment each.
pub const EPOCHS: usize = DICT_MAX / SEGMENT;

/// The training sample: the inputs in id order, up to the last whole input that fits in [`P16`], concatenated.
pub fn sample(training: &[Input]) -> Vec<u8> {
    let mut by_id: Vec<&Input> = training.iter().collect();
    by_id.sort_by(|a, b| a.id.cmp(&b.id));
    let mut s = Vec::new();
    for i in by_id {
        if s.len() + i.bytes.len() > P16 {
            break;
        }
        s.extend_from_slice(&i.bytes);
    }
    s
}

/// A hasher for d-mer keys: xxh3 of the key's 8 bytes, the same in every process.
#[derive(Default)]
struct DmerHash(u64);

impl Hasher for DmerHash {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = xxhash_rust::xxh3::xxh3_64_with_seed(bytes, self.0);
    }

    fn write_u64(&mut self, x: u64) {
        self.0 = xxhash_rust::xxh3::xxh3_64(&x.to_le_bytes());
    }
}

type Counts = HashMap<u64, u64, BuildHasherDefault<DmerHash>>;

/// The d-mer starting at `s[i]`.
fn dmer(s: &[u8], i: usize) -> u64 {
    let mut b = [0u8; D];
    b.copy_from_slice(&s[i..i + D]);
    u64::from_le_bytes(b)
}

/// The distinct d-mers of `chunk`, sorted.
fn distinct(chunk: &[u8]) -> Vec<u64> {
    let mut v: Vec<u64> = (0..chunk.len().saturating_sub(D - 1))
        .map(|i| dmer(chunk, i))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The epoch cover of `s` (see the module doc); `s` itself when it is at most [`DICT_MAX`] bytes.
pub fn cover(s: &[u8]) -> Vec<u8> {
    let l = s.len();
    if l <= DICT_MAX {
        return s.to_vec();
    }
    let mut counts = Counts::default();
    for i in 0..=l - D {
        *counts.entry(dmer(s, i)).or_insert(0) += 1;
    }
    // (score at choice time, position, length) of each epoch's choice.
    let mut chosen: Vec<(u64, usize, usize)> = Vec::with_capacity(EPOCHS);
    for e in 0..EPOCHS {
        let (lo, hi) = (e * l / EPOCHS, (e + 1) * l / EPOCHS);
        let mut best: Option<(u64, usize, usize, Vec<u64>)> = None;
        let mut at = lo;
        while at < hi {
            let len = SEGMENT.min(hi - at);
            let ds = distinct(&s[at..at + len]);
            let score: u64 = ds.iter().map(|d| counts.get(d).copied().unwrap_or(0)).sum();
            if best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, at, len, ds));
            }
            at += len;
        }
        let (score, pos, len, ds) = best.expect("an epoch holds at least one chunk");
        for d in &ds {
            if let Some(c) = counts.get_mut(d) {
                *c = 0;
            }
        }
        chosen.push((score, pos, len));
    }
    chosen.sort_unstable_by_key(|&(score, pos, _)| (score, pos));
    chosen
        .iter()
        .flat_map(|&(_, pos, len)| s[pos..pos + len].iter().copied())
        .collect()
}

/// The dictionary trained on `training`: [`cover`] of its [`sample`].
pub fn train(training: &[Input]) -> Vec<u8> {
    cover(&sample(training))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m6::corpus::{split, synthetic_notes};

    /// SplitMix64 bytes in 1..=255.
    fn fresh(seed: u64, n: usize) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = x;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                1 + ((z ^ (z >> 31)) % 255) as u8
            })
            .collect()
    }

    fn inputs(chunks: &[Vec<u8>]) -> Vec<Input> {
        chunks
            .iter()
            .enumerate()
            .map(|(i, c)| Input {
                id: format!("t/{i:03}"),
                bytes: c.clone(),
            })
            .collect()
    }

    /// The dictionary is at most 65,536 bytes, and a sample of at most 65,536 bytes is the dictionary as it is; the
    /// sample takes the training inputs in id order up to the last whole one that fits in P16 = 4 MiB.
    #[test]
    fn dictionary_is_at_most_64_kib() {
        let s = split(synthetic_notes(3)).unwrap();
        assert!(sample(&s.training).len() > DICT_MAX, "the cover path runs");
        let d = train(&s.training);
        assert!(!d.is_empty() && d.len() <= DICT_MAX, "{}", d.len());
        let small = inputs(&[b"tiny".to_vec(), b" and small".to_vec()]);
        assert_eq!(train(&small), b"tiny and small");
        let exact = inputs(&[fresh(1, DICT_MAX)]);
        assert_eq!(train(&exact), exact[0].bytes);
        let over = inputs(&[fresh(1, DICT_MAX + 1)]);
        assert!(train(&over).len() <= DICT_MAX);
        // P16: 70 inputs of 60,000 bytes; 69 fit in 4,194,304 bytes.
        let big: Vec<Input> = (0..70)
            .rev()
            .map(|i| Input {
                id: format!("b/{i:02}"),
                bytes: vec![i as u8; 60_000],
            })
            .collect();
        let smp = sample(&big);
        assert_eq!(smp.len(), 69 * 60_000);
        assert_eq!(
            (smp[0], smp[smp.len() - 1]),
            (0, 68),
            "id order, the last input left out"
        );
    }

    /// The dictionary is made of the training inputs' bytes only. The training split of the synthetic notes is in
    /// lower case and the measured split in upper case, so a measured byte or an invented one (a pad) shows: every
    /// byte of the dictionary occurs in the training sample, and none is an upper-case letter.
    #[test]
    fn dictionary_holds_training_bytes_only() {
        let mut corpus = synthetic_notes(5);
        for i in &mut corpus {
            if crate::m6::corpus::in_training(&i.id) {
                i.bytes.make_ascii_lowercase();
            } else {
                i.bytes.make_ascii_uppercase();
            }
        }
        let s = split(corpus).unwrap();
        let mut seen = [false; 256];
        for b in sample(&s.training) {
            seen[usize::from(b)] = true;
        }
        let d = train(&s.training);
        assert!(d.len() > DICT_MAX / 2 && d.len() <= DICT_MAX, "{}", d.len());
        assert!(!d.iter().any(u8::is_ascii_uppercase), "a measured byte");
        assert!(
            d.iter().all(|b| seen[usize::from(*b)]),
            "a byte not in the sample"
        );
    }

    /// The trainer's every rule is contested on a planted sample of L = 196,608 = 32 × 3 × 2,048 bytes, so epoch e
    /// is exactly chunks 3e, 3e + 1 and 3e + 2:
    ///
    /// - a fresh chunk: 2,048 seeded bytes in 1..=255, whose 2,041 d-mers occur once in S (score 2,041);
    /// - `Z`: 2,048 zero bytes, one distinct d-mer counted 2,041 times (distinct score 2,041; all windows 2,041²);
    /// - `B1`, `B2`, `B3`: fresh blocks placed twice each (distinct score 4,082 while both copies count).
    ///
    /// | Epoch | Chunks | Choice | What a wrong rule picks |
    /// |---|---|---|---|
    /// | 2 | fresh, `Z`, `B1` | `B1`, the best in the last place | all windows: `Z`; the first chunk: fresh |
    /// | 9 | `B1`, fresh, fresh | chunk 1: `B1` was zeroed, so the fresh chunks tie | no zeroing, or counts lowered to 1 (a tie, first): `B1`; ties to the last: chunk 2 |
    /// | 15 | `B2`, `B2`, fresh | chunk 0, a tie at 4,082 | ties to the last: chunk 1 |
    /// | 20 | fresh, `B3`, fresh | `B3`, the best in the middle | the first chunk: fresh |
    /// | 25 | fresh, fresh, `B3` | chunk 0: `B3` was zeroed | no zeroing: `B3` |
    /// | other | fresh, fresh, fresh | chunk 0, a tie at 2,041 | ties to the last: chunk 2; a hash-map order: any |
    ///
    /// The output is the 29 chosen chunks of score 2,041 in position order, then `B1`, `B2`, `B3` (score 4,082, by
    /// position): 65,536 bytes. Descending order puts `B3` first; epochs of 2,048 bytes from the start never reach `B3`.
    #[test]
    fn training_follows_the_epoch_cover() {
        let block = |k: u64| fresh(1_000 + k, SEGMENT);
        let (b1, b2, b3, z) = (block(1), block(2), block(3), vec![0u8; SEGMENT]);
        let mut chunks: Vec<Vec<u8>> = (0..96).map(|c| fresh(10_000 + c, SEGMENT)).collect();
        let mut place = |e: usize, j: usize, c: &Vec<u8>| chunks[3 * e + j] = c.clone();
        place(2, 1, &z);
        place(2, 2, &b1);
        place(9, 0, &b1);
        place(15, 0, &b2);
        place(15, 1, &b2);
        place(20, 1, &b3);
        place(25, 2, &b3);
        let training = inputs(&chunks);
        assert_eq!(sample(&training).len(), 196_608);
        // Epoch e's choice: chunk 3e, except epochs 2 (B1, chunk 8), 9 (the fresh chunk 28 after the zeroed B1) and
        // 20 (B3, chunk 61).
        let pick = |e: usize| match e {
            2 => 3 * e + 2,
            9 | 20 => 3 * e + 1,
            _ => 3 * e,
        };
        let high = [2, 15, 20];
        let mut expect: Vec<u8> = (0..EPOCHS)
            .filter(|e| !high.contains(e))
            .flat_map(|e| chunks[pick(e)].clone())
            .collect();
        for e in high {
            expect.extend_from_slice(&chunks[pick(e)]);
        }
        assert_eq!(expect.len(), DICT_MAX);
        for _ in 0..2 {
            let d = train(&training);
            assert!(
                d == expect,
                "the cover differs from the hand-derived dictionary"
            );
        }
        assert_eq!(&expect[DICT_MAX - 3 * SEGMENT..DICT_MAX - 2 * SEGMENT], b1);
        assert_eq!(&expect[DICT_MAX - SEGMENT..], b3);
    }

    /// The dictionary helps: on the synthetic notes, lz4 with the dictionary trained on the training split stores
    /// the measured split in fewer bytes than lz4 without one.
    #[test]
    fn dictionary_shrinks_a_redundant_corpus() {
        let s = split(synthetic_notes(9)).unwrap();
        let d = train(&s.training);
        let (mut plain, mut with) = (0usize, 0usize);
        for i in &s.measured {
            plain += lz4_flex::block::compress(&i.bytes).len();
            with += lz4_flex::block::compress_with_dict(&i.bytes, &d).len();
        }
        assert!(with < plain, "with {with} >= without {plain}");
        assert!(
            with * 20 < plain * 19,
            "at least 5 % smaller: {with} vs {plain}"
        );
    }
}
