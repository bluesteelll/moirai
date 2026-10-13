//! Measurement 6 (WP-54, [60 §5.2] item 6, `60-roadmap.md:762`): the body and `hist` codecs. It measures size, encode
//! and decode speed and RSS of `lz4_flex` 0.14 block with and without a raw-content dictionary of at most 64 KiB, of
//! `ruzstd` 0.9 at `Fastest`, and of the `zstd` CLI at level 1 as option (3)'s proxy, on the owner's notes, on the plan
//! sections of `docs/**` and on `hist` frames (`90-harness-agnostic-design.md:959-974`). It decides
//! `F02-dict-file`, `F10-codec-values`, `-blob-codec`, `-hist-codec`, `-dict-form` and `-body-placement`
//! (`docs/spec/HOLES.md:33-39`).
//!
//! **The corpora** ([`corpus`]): the notes through `/private/codec-bodies/manifest.tsv`, the plan sections split at
//! headings outside fences, where a recording run and a smoke find them, and the split of each corpus into a training
//! and a measured side, keyed by each input's id.
//!
//! **The dictionary** ([`dict`]): the probe's own deterministic trainer (R-33 Q1 (b)), the epoch cover of `ruzstd`'s
//! `dict_builder` with exact d-mer counts, over the training side only; at most 64 KiB of raw content. A product
//! trainer may do better or worse, so the gain the dictionary shows is this trainer's.
//!
//! **The `hist` frames** ([`frames`]): the records of four synthetic tails of measurement 10's `rec` family, without
//! their rotation pads, cut into frames by F10 §4.2; split frames of P04-byte blocks. They follow the generator, not real
//! logs.
//!
//! **The codecs** ([`codec`]): `lz4_flex` 0.14 block with and without the dictionary and `ruzstd` 0.9 at `Fastest`, each
//! an instance with its own buffers, and the `zstd` CLI's stage below the working directory, found on `PATH` only.
//!
//! **The grid** ([`quantities`], names by [MP §4.1]): 27 quantities, and 2 more when a `zstd` CLI is on `PATH`, in run
//! order with every size first (R-34 B-3, M-15; R-35 Q3):
//! - `raw.<c>` and `size.<codec>.<c>` for the notes and the plan sections (`lz4`, `lz4d`, `zstd`), and for `hist`
//!   (`lz4`, `zstd`: a `hist` frame never uses a dictionary, `10-sealed-files.md:137`); then `size.cli.notes` and
//!   `size.cli.plan` when the CLI is present;
//! - then `dec.<codec>.<c>` and `enc.<codec>.<c>` for the notes, the plan sections and `hist`.
//!
//! Each quantity has one arm, arm 0, named by its first token; no later arm exists, since every later arm would be a
//! floor ([MP §4.1]) and measurement 6 has none. No quantity declares a minimum tier.
//!
//! **The arms.** `raw` is the measured side's raw bytes. `size` is the payload F10 would store, summed over the
//! measured side: per input, or per `hist` frame, the codec's output, or the raw bytes when the output is not shorter
//! (`10-sealed-files.md:135-141`); every sample encodes the side again. `enc` and `dec` time one input of the measured
//! side, cycled in id order, or with a batch of k ([MP §4.5]) k consecutive inputs and their mean ([`Batched`]): the
//! untimed setup picks the inputs, the timed operation is exactly the k codec calls into the arm's own buffers, and the
//! untimed teardown checks each call's output ([`codec::Codec::check`]). The CLI's size is its time-free proxy only:
//! its time is a spawn and file I/O.
//!
//! **The setup** ([`prepare`], before any arm exists): the corpus load, the split, the dictionary's training (on the
//! training side only), the `hist` cut, and for every in-process codec quantity a round trip of every measured input,
//! which fails the run naming the codec and the input unless the decoded bytes equal the raw ones. A `dec` quantity
//! keeps the inputs it encoded. So every run checks its codec on the corpus it measures.
//!
//! **RSS.** A record's `private_peak` includes the setup (a `dec` quantity's encoder with it); `heap_high_water` is
//! reset before the first pilot, so it is the run's own. Neither is under 90 §11.3's decision rule.

pub mod codec;
pub mod corpus;
pub mod dict;
pub mod frames;

use crate::arm::{Arm, ArmError, Ticker, ValueFn};
use crate::units::Unit;
use codec::{CliStage, Codec, CodecKind, Op, stored};
use corpus::{Input, split};
use frames::Frames;
use std::cell::RefCell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// The measurement's row number.
pub const MEASUREMENT: u32 = 6;

/// A corpus of measurement 6.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Corpus {
    /// The owner's notes.
    Notes,
    /// The plan sections of `docs/**`.
    Plan,
    /// The `hist` frames.
    Hist,
}

impl Corpus {
    /// The name in a quantity.
    pub fn name(self) -> &'static str {
        match self {
            Corpus::Notes => "notes",
            Corpus::Plan => "plan",
            Corpus::Hist => "hist",
        }
    }
}

/// What a quantity measures.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Measure {
    /// The measured side's raw bytes.
    Raw,
    /// The stored payload of an in-process codec.
    Size(CodecKind),
    /// The stored payload of the `zstd` CLI at level 1 with the dictionary.
    CliSize,
    /// The time of one encode.
    Enc(CodecKind),
    /// The time of one decode.
    Dec(CodecKind),
}

impl Measure {
    /// The in-process codec, if any.
    pub fn codec(self) -> Option<CodecKind> {
        match self {
            Measure::Size(k) | Measure::Enc(k) | Measure::Dec(k) => Some(k),
            Measure::Raw | Measure::CliSize => None,
        }
    }

    /// Whether it needs the dictionary.
    pub fn needs_dict(self) -> bool {
        self == Measure::CliSize || self.codec() == Some(CodecKind::Lz4Dict)
    }
}

/// One quantity of the grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    /// The name, `<measure>[.<codec>].<corpus>`.
    pub name: String,
    /// What it measures.
    pub measure: Measure,
    /// On which corpus.
    pub corpus: Corpus,
}

impl Quantity {
    fn new(measure: Measure, corpus: Corpus) -> Quantity {
        let m = match measure {
            Measure::Raw => "raw".to_string(),
            Measure::Size(k) => format!("size.{}", k.name()),
            Measure::CliSize => "size.cli".to_string(),
            Measure::Enc(k) => format!("enc.{}", k.name()),
            Measure::Dec(k) => format!("dec.{}", k.name()),
        };
        Quantity {
            name: format!("{m}.{}", corpus.name()),
            measure,
            corpus,
        }
    }

    /// The name of its one arm: the name's first token.
    pub fn arm_name(&self) -> &'static str {
        match self.measure {
            Measure::Raw => "raw",
            Measure::Size(_) | Measure::CliSize => "size",
            Measure::Enc(_) => "enc",
            Measure::Dec(_) => "dec",
        }
    }
}

/// The codecs measured on a corpus: every in-process codec, but no dictionary on `hist`.
fn codecs(c: Corpus) -> &'static [CodecKind] {
    match c {
        Corpus::Hist => &[CodecKind::Lz4, CodecKind::Zstd],
        _ => &[CodecKind::Lz4, CodecKind::Lz4Dict, CodecKind::Zstd],
    }
}

/// The grid in run order: every size first, the CLI's two after `size.zstd.hist` when `cli`, then the decode and
/// encode times (R-34 M-15, R-35 Q3).
pub fn quantities(cli: bool) -> Vec<Quantity> {
    let all = [Corpus::Notes, Corpus::Plan, Corpus::Hist];
    let mut g = Vec::new();
    for c in all {
        g.push(Quantity::new(Measure::Raw, c));
        for &k in codecs(c) {
            g.push(Quantity::new(Measure::Size(k), c));
        }
    }
    if cli {
        for c in [Corpus::Notes, Corpus::Plan] {
            g.push(Quantity::new(Measure::CliSize, c));
        }
    }
    for c in all {
        for &k in codecs(c) {
            g.push(Quantity::new(Measure::Dec(k), c));
        }
        for &k in codecs(c) {
            g.push(Quantity::new(Measure::Enc(k), c));
        }
    }
    g
}

/// Makes codec instances: [`codec::make`] in a run, a spy in the tests.
pub trait Factory {
    /// A codec of `kind` for inputs of at most `max_input` bytes; `lz4d` takes `dict`.
    fn make<'a>(&self, kind: CodecKind, dict: &[u8], max_input: usize) -> Box<dyn Codec<'a> + 'a>;
}

/// The real codecs.
pub struct RealCodecs;

impl Factory for RealCodecs {
    fn make<'a>(&self, kind: CodecKind, dict: &[u8], max_input: usize) -> Box<dyn Codec<'a> + 'a> {
        codec::make(kind, dict, max_input)
    }
}

/// The setup's seams, so the tests can put spies on the shared clock.
pub struct Seams<'s> {
    /// A corpus: for the notes and the plan every input as a frame of its own, for `hist` the cut frames.
    pub load: &'s dyn Fn(Corpus) -> Result<Frames, String>,
    /// The dictionary's trainer over the training side ([`dict::train`]).
    pub train: &'s dyn Fn(&[Input]) -> Vec<u8>,
    /// The codecs.
    pub codec: &'s dyn Factory,
}

/// What [`prepare`] made for one quantity's arm.
#[derive(Clone, Debug, Default)]
pub struct Prepared {
    /// The measured inputs in id order: the measured side, or every `hist` block.
    pub inputs: Vec<Input>,
    /// The frames over `inputs`: each input alone, or the `hist` frames.
    pub frames: Vec<Range<usize>>,
    /// The dictionary, when the quantity needs one.
    pub dict: Vec<u8>,
    /// A `dec` quantity's inputs, encoded by its codec in the setup.
    pub encoded: Vec<Vec<u8>>,
    /// The largest input.
    pub max_input: usize,
}

/// The setup of `q` (before any arm exists): the corpus, the split (not for `hist`), the dictionary trained on the
/// training side, and for an in-process codec a round trip of every measured input.
pub fn prepare(q: &Quantity, seams: &Seams<'_>) -> Result<Prepared, String> {
    let loaded = (seams.load)(q.corpus)?;
    let (inputs, frames, training) = if q.corpus == Corpus::Hist {
        (loaded.blocks, loaded.frames, Vec::new())
    } else {
        let s = split(loaded.blocks)?;
        let frames = (0..s.measured.len()).map(|i| i..i + 1).collect();
        (s.measured, frames, s.training)
    };
    let dict = if q.measure.needs_dict() {
        (seams.train)(&training)
    } else {
        Vec::new()
    };
    let max_input = inputs.iter().map(|i| i.bytes.len()).max().unwrap_or(0);
    let mut encoded = Vec::new();
    if let Some(kind) = q.measure.codec() {
        let mut c = seams.codec.make(kind, &dict, max_input);
        let mut out = vec![0u8; max_input];
        for i in &inputs {
            let enc = c
                .encode(&i.bytes)
                .map_err(|e| format!("{} {}: {e}", kind.name(), i.id))?
                .to_vec();
            let raw = i.bytes.len();
            let n = c
                .decode(&enc, &mut out[..raw])
                .map_err(|e| format!("{} {}: {e}", kind.name(), i.id))?;
            if n != raw || out[..raw] != i.bytes[..] {
                return Err(format!(
                    "{}: the round trip of {} differs from its raw bytes",
                    kind.name(),
                    i.id
                ));
            }
            if matches!(q.measure, Measure::Dec(_)) {
                encoded.push(enc);
            }
        }
    }
    Ok(Prepared {
        inputs,
        frames,
        dict,
        encoded,
        max_input,
    })
}

/// A batchable time arm in three stages ([MP §4.1], [MP §4.5]): each sample runs `setup(k)` untimed, times `op` alone,
/// runs `teardown` untimed on its result, and returns the timed span divided by the batch k. The pattern of
/// [`crate::m14::Staged`], with batches.
pub struct Batched<S, P, D, T> {
    name: &'static str,
    setup: S,
    op: P,
    teardown: D,
    ticker: T,
    batch: u32,
}

impl<S, P, D, T> Batched<S, P, D, T> {
    /// A time arm named `name`.
    pub fn new(name: &'static str, ticker: T, setup: S, op: P, teardown: D) -> Self {
        Batched {
            name,
            setup,
            op,
            teardown,
            ticker,
            batch: 1,
        }
    }
}

impl<S, P, D, T, X, O> Arm for Batched<S, P, D, T>
where
    S: FnMut(u32) -> Result<X, ArmError>,
    P: FnMut(X) -> Result<O, ArmError>,
    D: FnMut(O) -> Result<(), ArmError>,
    T: Ticker,
{
    fn name(&self) -> &str {
        self.name
    }

    fn unit(&self) -> Unit {
        Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let x = (self.setup)(self.batch)?;
        let t0 = self.ticker.now_ns();
        let o = (self.op)(x)?;
        let t1 = self.ticker.now_ns();
        (self.teardown)(std::hint::black_box(o))?;
        Ok(t1.saturating_sub(t0) / u64::from(self.batch))
    }

    fn set_batch(&mut self, k: u32) {
        self.batch = k.max(1);
    }

    fn batch(&self) -> u32 {
        self.batch
    }
}

/// Where the CLI quantities run: the `zstd` executable found on `PATH`, and the working directory.
#[derive(Clone, Debug, Default)]
pub struct Cli {
    /// The executable, when one is on `PATH`.
    pub zstd: Option<PathBuf>,
    /// The working directory, below which its stage lies.
    pub work: PathBuf,
}

fn arm_err(e: String) -> ArmError {
    ArmError(e)
}

/// The stored payload of every frame of `p` with `c`: per frame, the codec's outputs summed, or the frame's raw bytes
/// when they are not shorter (`10-sealed-files.md:135-141`).
fn stored_size<'a>(c: &mut dyn Codec<'a>, p: &'a Prepared) -> Result<u64, ArmError> {
    let mut total = 0u64;
    for f in &p.frames {
        let (mut raw, mut enc) = (0usize, 0usize);
        for i in &p.inputs[f.clone()] {
            raw += i.bytes.len();
            enc += c.encode(&i.bytes).map_err(arm_err)?.len();
        }
        total += stored(raw, enc) as u64;
    }
    Ok(total)
}

/// The arm of `q` over `p` (made before any sample, untimed): a value arm for `raw` and `size`, a [`Batched`] time
/// arm for `enc` and `dec`. Of the seams it uses only the codecs: the corpus and the dictionary are `p`'s.
pub fn arm<'a, T: Ticker + 'a>(
    q: &Quantity,
    p: &'a Prepared,
    seams: &'a Seams<'a>,
    cli: &Cli,
    ticker: T,
) -> Result<Box<dyn Arm + 'a>, String> {
    let name = q.arm_name();
    match q.measure {
        Measure::Raw => {
            let raw: u64 = p.inputs.iter().map(|i| i.bytes.len() as u64).sum();
            Ok(Box::new(ValueFn::new(name, Unit::Bytes, move || Ok(raw))))
        }
        Measure::Size(kind) => {
            let mut c = seams.codec.make(kind, &p.dict, p.max_input);
            Ok(Box::new(ValueFn::new(name, Unit::Bytes, move || {
                stored_size(c.as_mut(), p)
            })))
        }
        Measure::CliSize => {
            let zstd = cli
                .zstd
                .clone()
                .ok_or_else(|| format!("{} needs a zstd executable on PATH", q.name))?;
            let inputs: Vec<&[u8]> = p.inputs.iter().map(|i| i.bytes.as_slice()).collect();
            let stage = CliStage::new(&cli.work, &p.dict, &inputs)?;
            Ok(Box::new(ValueFn::new(name, Unit::Bytes, move || {
                let sizes = stage.compress(&zstd).map_err(arm_err)?;
                Ok(sizes
                    .iter()
                    .zip(&p.inputs)
                    .map(|(&z, i)| stored(i.bytes.len(), z as usize) as u64)
                    .sum())
            })))
        }
        Measure::Enc(kind) | Measure::Dec(kind) => {
            let op = if matches!(q.measure, Measure::Enc(_)) {
                Op::Encode
            } else {
                Op::Decode
            };
            if p.inputs.is_empty() {
                return Err(format!("{}: no measured input", q.name));
            }
            Ok(time_arm(name, op, kind, p, seams, ticker))
        }
    }
}

/// The `enc` or `dec` arm: the setup takes the next k inputs in id order, the timed operation makes exactly k codec
/// calls into the codec's and the arm's buffers, and the teardown checks each output.
fn time_arm<'a, T: Ticker + 'a>(
    name: &'static str,
    op: Op,
    kind: CodecKind,
    p: &'a Prepared,
    seams: &'a Seams<'a>,
    ticker: T,
) -> Box<dyn Arm + 'a> {
    let codec = Rc::new(RefCell::new(seams.codec.make(kind, &p.dict, p.max_input)));
    let (timed, check) = (codec.clone(), codec);
    let n = p.inputs.len();
    let mut next = 0usize;
    let mut out = vec![0u8; p.max_input];
    let setup = move |k: u32| -> Result<(Vec<usize>, Vec<usize>), ArmError> {
        let picked: Vec<usize> = (0..k as usize).map(|j| (next + j) % n).collect();
        next = (next + k as usize) % n;
        Ok((picked, Vec::with_capacity(k as usize)))
    };
    let run = move |(picked, mut lens): (Vec<usize>, Vec<usize>)| {
        let mut c = timed.borrow_mut();
        for &i in &picked {
            let raw = &p.inputs[i].bytes;
            let len = match op {
                Op::Encode => c.encode(raw).map_err(arm_err)?.len(),
                Op::Decode => c
                    .decode(&p.encoded[i], &mut out[..raw.len()])
                    .map_err(arm_err)?,
            };
            lens.push(len);
        }
        Ok((picked, lens))
    };
    let teardown = move |(picked, lens): (Vec<usize>, Vec<usize>)| {
        let c = check.borrow();
        for (&i, &len) in picked.iter().zip(&lens) {
            c.check(op, p.inputs[i].bytes.len(), len).map_err(arm_err)?;
        }
        Ok(())
    };
    Box::new(Batched::new(name, ticker, setup, run, teardown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::valid_name;
    use crate::m6::corpus::{in_training, synthetic_notes, synthetic_plan};
    use crate::tail::Params;
    use crate::testkit::FakeTicker;
    use std::cell::Cell;
    use std::collections::BTreeSet;

    const SEC: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;

    /// One call the spy codec saw.
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Call {
        op: Op,
        bytes: Vec<u8>,
    }

    /// Run-length coding: (count, byte) pairs; repeated bytes shrink, others double.
    fn rle(b: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let mut n = 1;
            while i + n < b.len() && b[i + n] == b[i] && n < 255 {
                n += 1;
            }
            v.extend([n as u8, b[i]]);
            i += n;
        }
        v
    }

    fn unrle(e: &[u8]) -> Vec<u8> {
        e.as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[n, b]| std::iter::repeat_n(b, usize::from(n)))
            .collect()
    }

    /// A spy codec on the shared clock: each encode or decode takes 100 + (its input's length) ns and is logged; the
    /// teardown's check takes 3 ms; its coding is [`rle`]. With `flip`, a decode whose output starts with `FLIP`
    /// changes its first byte.
    struct Spy {
        kind: CodecKind,
        clock: FakeTicker,
        log: Rc<RefCell<Vec<Call>>>,
        out: Vec<u8>,
        flip: bool,
    }

    impl<'a> Codec<'a> for Spy {
        fn kind(&self) -> CodecKind {
            self.kind
        }

        fn encode(&mut self, input: &'a [u8]) -> Result<&[u8], String> {
            self.clock.advance(100 + input.len() as u64);
            self.log.borrow_mut().push(Call {
                op: Op::Encode,
                bytes: input.to_vec(),
            });
            self.out = rle(input);
            Ok(&self.out)
        }

        fn decode(&mut self, enc: &[u8], out: &mut [u8]) -> Result<usize, String> {
            self.clock.advance(100 + enc.len() as u64);
            self.log.borrow_mut().push(Call {
                op: Op::Decode,
                bytes: enc.to_vec(),
            });
            let d = unrle(enc);
            let n = d.len().min(out.len());
            out[..n].copy_from_slice(&d[..n]);
            if self.flip && out.starts_with(b"FLIP") {
                out[0] ^= 1;
            }
            Ok(n)
        }

        fn check(&self, op: Op, raw_len: usize, out_len: usize) -> Result<(), String> {
            self.clock.advance(3 * MS);
            match op {
                Op::Decode if out_len != raw_len => Err("short decode".into()),
                _ => Ok(()),
            }
        }
    }

    /// The spy factory: each codec it makes costs 5 ms of the clock.
    struct Spies {
        clock: FakeTicker,
        log: Rc<RefCell<Vec<Call>>>,
        made: Cell<usize>,
        flip: bool,
    }

    impl Factory for Spies {
        fn make<'a>(&self, kind: CodecKind, _dict: &[u8], _max: usize) -> Box<dyn Codec<'a> + 'a> {
            self.clock.advance(5 * MS);
            self.made.set(self.made.get() + 1);
            Box::new(Spy {
                kind,
                clock: self.clock.clone(),
                log: self.log.clone(),
                out: Vec::new(),
                flip: self.flip,
            })
        }
    }

    fn spies(clock: &FakeTicker) -> Spies {
        Spies {
            clock: clock.clone(),
            log: Rc::default(),
            made: Cell::new(0),
            flip: false,
        }
    }

    /// Every input a frame of its own.
    fn singles(blocks: Vec<Input>) -> Frames {
        let frames = (0..blocks.len()).map(|i| i..i + 1).collect();
        Frames { blocks, frames }
    }

    /// The test corpora: the synthetic notes and plan sections, and the `hist` frames at the test profile.
    fn corpus(c: Corpus) -> Result<Frames, String> {
        match c {
            Corpus::Notes => Ok(singles(synthetic_notes(21))),
            Corpus::Plan => Ok(singles(synthetic_plan(21))),
            Corpus::Hist => frames::hist(Params::TEST),
        }
    }

    /// The measured side of `c` as the test corpora give it, in id order, by the split's own rule.
    fn measured(c: Corpus) -> Vec<Input> {
        let mut v = corpus(c).unwrap().blocks;
        if c != Corpus::Hist {
            v.retain(|i| !in_training(&i.id));
            v.sort_by(|a, b| a.id.cmp(&b.id));
        }
        v
    }

    fn time_quantities() -> Vec<Quantity> {
        quantities(false)
            .into_iter()
            .filter(|q| matches!(q.measure, Measure::Enc(_) | Measure::Dec(_)))
            .collect()
    }

    /// 27 quantities without the CLI and 29 with it, valid and unique names, in the run order of R-34 M-15 and R-35
    /// Q3: every size first, the CLI's after `size.zstd.hist`, then the decode and encode times; each arm named by
    /// its quantity's first token; no dictionary codec on `hist`.
    #[test]
    fn grid_names_are_valid_unique_and_ordered() {
        let names = |cli| -> Vec<String> { quantities(cli).into_iter().map(|q| q.name).collect() };
        let want = [
            "raw.notes",
            "size.lz4.notes",
            "size.lz4d.notes",
            "size.zstd.notes",
            "raw.plan",
            "size.lz4.plan",
            "size.lz4d.plan",
            "size.zstd.plan",
            "raw.hist",
            "size.lz4.hist",
            "size.zstd.hist",
            "dec.lz4.notes",
            "dec.lz4d.notes",
            "dec.zstd.notes",
            "enc.lz4.notes",
            "enc.lz4d.notes",
            "enc.zstd.notes",
            "dec.lz4.plan",
            "dec.lz4d.plan",
            "dec.zstd.plan",
            "enc.lz4.plan",
            "enc.lz4d.plan",
            "enc.zstd.plan",
            "dec.lz4.hist",
            "dec.zstd.hist",
            "enc.lz4.hist",
            "enc.zstd.hist",
        ];
        assert_eq!(names(false), want);
        let mut with = want.map(String::from).to_vec();
        with.splice(11..11, ["size.cli.notes".into(), "size.cli.plan".into()]);
        assert_eq!(names(true), with);
        for q in quantities(true) {
            assert!(valid_name(&q.name), "{}", q.name);
            assert_eq!(q.name.split('.').next(), Some(q.arm_name()));
            assert!(valid_name(q.arm_name()));
            assert_eq!(q, Quantity::new(q.measure, q.corpus));
            assert!(
                q.corpus != Corpus::Hist || !q.measure.needs_dict(),
                "{}",
                q.name
            );
        }
        let set: BTreeSet<String> = names(true).into_iter().collect();
        assert_eq!(set.len(), 29);
    }

    /// The batched arm times only its operation: setup and teardown ticks stay out, and the sample is the timed
    /// span divided by the batch; the setup is told the batch.
    #[test]
    fn batched_arm_times_only_its_operation() {
        let clock = FakeTicker::manual();
        let (a, b, c) = (clock.clone(), clock.clone(), clock.clone());
        let seen = Rc::new(Cell::new(0u32));
        let s = seen.clone();
        let mut arm = Batched::new(
            "enc",
            clock.clone(),
            move |k: u32| {
                s.set(k);
                a.advance(7_000);
                Ok(k)
            },
            move |k: u32| {
                b.advance(1_000 * u64::from(k) + 3);
                Ok(k)
            },
            move |_: u32| {
                c.advance(5_000);
                Ok(())
            },
        );
        assert_eq!((arm.name(), arm.unit(), arm.batch()), ("enc", Unit::Ns, 1));
        assert_eq!(arm.sample().unwrap(), 1_003);
        arm.set_batch(4);
        assert_eq!(arm.batch(), 4);
        assert_eq!(arm.sample().unwrap(), 1_000, "(4,000 + 3) / 4");
        assert_eq!(seen.get(), 4);
        arm.set_batch(0);
        assert_eq!(arm.batch(), 1);
    }

    /// Every `enc` and `dec` arm times exactly its codec calls on the measured side, cycled in id order: with a
    /// batch of 1 each sample is the spy's 100 + (input length) ns of the next input, and the spy saw exactly that
    /// input (a `dec` arm its encoded form, never the raw one); with a batch of 3, the mean of 3 consecutive inputs.
    #[test]
    fn every_time_arm_times_its_codec_on_the_measured_split() {
        for q in time_quantities() {
            let clock = FakeTicker::manual();
            let f = spies(&clock);
            let seams = Seams {
                load: &corpus,
                train: &dict::train,
                codec: &f,
            };
            let p = prepare(&q, &seams).unwrap();
            let want = measured(q.corpus);
            assert_eq!(p.inputs, want, "{}", q.name);
            let op = if matches!(q.measure, Measure::Enc(_)) {
                Op::Encode
            } else {
                Op::Decode
            };
            let sent = |i: &Input| match op {
                Op::Encode => i.bytes.clone(),
                Op::Decode => rle(&i.bytes),
            };
            let mut arm = arm(&q, &p, &seams, &Cli::default(), clock.clone()).unwrap();
            f.log.borrow_mut().clear();
            let n = want.len();
            for j in 0..n + 2 {
                let i = &want[j % n];
                assert_eq!(
                    arm.sample().unwrap(),
                    100 + sent(i).len() as u64,
                    "{} sample {j}",
                    q.name
                );
                let log = std::mem::take(&mut *f.log.borrow_mut());
                assert_eq!(log, [Call { op, bytes: sent(i) }], "{} sample {j}", q.name);
            }
            arm.set_batch(3);
            let start = (n + 2) % n;
            let three: u64 = (0..3)
                .map(|j| 100 + sent(&want[(start + j) % n]).len() as u64)
                .sum();
            assert_eq!(arm.sample().unwrap(), three / 3, "{}", q.name);
        }
    }

    /// Setup and teardown stay outside every timed region: the spy loader takes 2 s, the spy trainer 1 s, each codec
    /// made 5 ms and each teardown check 3 ms of the shared clock, and yet every sample of every `enc` and `dec` arm
    /// is exactly the spy codec's call time. The trainer runs once, on exactly the training side, and only for a
    /// quantity that needs the dictionary.
    #[test]
    fn setup_and_teardown_stay_outside_every_timed_region() {
        for q in time_quantities() {
            let clock = FakeTicker::manual();
            let f = spies(&clock);
            let (c1, c2) = (clock.clone(), clock.clone());
            let trained: RefCell<Vec<Vec<String>>> = RefCell::default();
            let load = move |c: Corpus| {
                c1.advance(2 * SEC);
                corpus(c)
            };
            let train = |t: &[Input]| {
                c2.advance(SEC);
                trained
                    .borrow_mut()
                    .push(t.iter().map(|i| i.id.clone()).collect());
                dict::train(t)
            };
            let seams = Seams {
                load: &load,
                train: &train,
                codec: &f,
            };
            let mut before = clock.clone();
            let t0 = before.now_ns();
            let p = prepare(&q, &seams).unwrap();
            let mut arm = arm(&q, &p, &seams, &Cli::default(), clock.clone()).unwrap();
            assert!(
                before.now_ns() - t0 >= 2 * SEC,
                "the loader ran in the setup"
            );
            let calls = trained.borrow().clone();
            if q.measure.needs_dict() {
                let mut ids: Vec<String> = corpus(q.corpus)
                    .unwrap()
                    .blocks
                    .into_iter()
                    .map(|i| i.id)
                    .filter(|id| in_training(id))
                    .collect();
                ids.sort();
                assert_eq!(
                    calls,
                    [ids],
                    "{}: the trainer saw the training side",
                    q.name
                );
            } else {
                assert!(calls.is_empty(), "{}", q.name);
            }
            let want = measured(q.corpus);
            for (j, i) in want.iter().enumerate().take(5) {
                let sent = match q.measure {
                    Measure::Enc(_) => i.bytes.len(),
                    _ => rle(&i.bytes).len(),
                };
                assert_eq!(
                    arm.sample().unwrap(),
                    100 + sent as u64,
                    "{} sample {j}",
                    q.name
                );
            }
            assert_eq!(
                f.made.get(),
                2,
                "{}: the round trip's codec and the arm's",
                q.name
            );
        }
    }

    /// `raw` is the measured side's raw bytes, and `size` the stored payload of F10 §3.4 per input, and per `hist`
    /// frame: a split frame whose blocks are not together shorter is stored raw even when one block shrinks.
    #[test]
    fn size_and_raw_arms_sum_the_measured_split() {
        let clock = FakeTicker::manual();
        let f = spies(&clock);
        let runs = vec![b'a'; 4_096];
        let noisy: Vec<u8> = (0..4_096u32).map(|i| (i % 251) as u8).collect();
        let tail = vec![b'z'; 1_000];
        let planted = |c: Corpus| match c {
            Corpus::Hist => Ok(Frames {
                blocks: [
                    ("hist/0/000000.0000", &runs),
                    ("hist/0/000000.0001", &noisy),
                    ("hist/0/000001.0000", &tail),
                ]
                .into_iter()
                .map(|(id, b)| Input {
                    id: id.into(),
                    bytes: b.clone(),
                })
                .collect(),
                frames: vec![0..2, 2..3],
            }),
            c => corpus(c),
        };
        let seams = Seams {
            load: &planted,
            train: &dict::train,
            codec: &f,
        };
        let size = |q: &Quantity| -> u64 {
            let p = prepare(q, &seams).unwrap();
            let mut a = arm(q, &p, &seams, &Cli::default(), clock.clone()).unwrap();
            assert_eq!(a.unit(), Unit::Bytes);
            let v = a.sample().unwrap();
            assert_eq!(a.sample().unwrap(), v, "{}: every sample the same", q.name);
            v
        };
        for c in [Corpus::Notes, Corpus::Plan] {
            let m = measured(c);
            let raw: u64 = m.iter().map(|i| i.bytes.len() as u64).sum();
            let rle_stored: u64 = m
                .iter()
                .map(|i| rle(&i.bytes).len().min(i.bytes.len()) as u64)
                .sum();
            assert_eq!(size(&Quantity::new(Measure::Raw, c)), raw);
            for &k in codecs(c) {
                assert_eq!(
                    size(&Quantity::new(Measure::Size(k), c)),
                    rle_stored,
                    "{k:?} {c:?}"
                );
            }
        }
        assert_eq!(
            size(&Quantity::new(Measure::Raw, Corpus::Hist)),
            4_096 + 4_096 + 1_000
        );
        // Frame 0: rle(runs) is 34 bytes and rle(noisy) 8,192, together above its 8,192 raw: stored raw. Frame 1:
        // rle(tail) is 8 bytes.
        assert_eq!(
            (rle(&runs).len(), rle(&noisy).len(), rle(&tail).len()),
            (34, 8_192, 8)
        );
        for &k in codecs(Corpus::Hist) {
            assert_eq!(
                size(&Quantity::new(Measure::Size(k), Corpus::Hist)),
                8_192 + 8
            );
        }
    }

    /// The setup round-trips every measured input of every in-process codec quantity: a codec whose decode changes
    /// one byte of the input `FLIP-…` fails `prepare`, naming the codec and that input; `raw` does not round-trip.
    #[test]
    fn prepare_round_trips_every_measured_input() {
        let id = (0..)
            .map(|k| format!("flip-{k}"))
            .find(|id| !in_training(id))
            .unwrap();
        let notes = |c: Corpus| match c {
            Corpus::Notes => {
                let mut v = synthetic_notes(21);
                v.push(Input {
                    id: id.clone(),
                    bytes: b"FLIP-ME and some words after".to_vec(),
                });
                Ok(singles(v))
            }
            c => corpus(c),
        };
        let clock = FakeTicker::manual();
        let mut f = spies(&clock);
        f.flip = true;
        let seams = Seams {
            load: &notes,
            train: &dict::train,
            codec: &f,
        };
        for q in quantities(false)
            .iter()
            .filter(|q| q.corpus == Corpus::Notes)
        {
            let r = prepare(q, &seams);
            match q.measure.codec() {
                Some(k) => {
                    let e = r.unwrap_err();
                    assert!(e.contains(k.name()) && e.contains(&id), "{}: {e}", q.name);
                }
                None => assert!(r.is_ok(), "{}", q.name),
            }
        }
        f.flip = false;
        let seams = Seams {
            load: &notes,
            train: &dict::train,
            codec: &f,
        };
        let q = Quantity::new(Measure::Dec(CodecKind::Zstd), Corpus::Notes);
        let p = prepare(&q, &seams).unwrap();
        assert_eq!(p.encoded.len(), p.inputs.len());
        assert!(
            p.inputs
                .iter()
                .zip(&p.encoded)
                .all(|(i, e)| *e == rle(&i.bytes))
        );
        // The real codecs pass the same round trip on the test corpora.
        let real = Seams {
            load: &corpus,
            train: &dict::train,
            codec: &RealCodecs,
        };
        for q in quantities(false)
            .iter()
            .filter(|q| matches!(q.measure, Measure::Dec(_)))
        {
            let p = prepare(q, &real).unwrap();
            assert_eq!(p.encoded.len(), p.inputs.len(), "{}", q.name);
        }
    }
}
