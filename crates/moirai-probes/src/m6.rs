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
//!
//! **The driver** ([`cli`], `moirai-probes-bin m6`; [MP §9.7] through the shared [`driver`]): `--host`, `--condition`,
//! `--load-log`, `--quantity`, `--commit` and `--toolchain` (required), `--out` (required unless `--smoke`, refused
//! with it, created when missing), `--smoke`, `--help`. Without `--quantity`, each quantity runs in a child process
//! of its own ([MP §4.6]), the CLI's two only when a `zstd` is on `PATH`.
//! - **A recording run** of a notes, plan or `size.cli` quantity finds its corpus from `--out`, which must be
//!   `<P>/measurements/6`: the notes in `P/codec-bodies/`, the plan sections in `docs/` beside `P` ([`corpus::from_out`]);
//!   another `--out` fails naming it. A `hist` quantity reads no corpus and takes any `--out` (R-35 Q6). A `size.cli`
//!   quantity with no `zstd` on `PATH` fails.
//! - **The smoke** runs each requested quantity's arm once and records nothing. Its notes and plan sections are the
//!   build checkout's when present ([`corpus::from_checkout`]), else synthetic, and one line per corpus says which with
//!   its path and counts, never its text; `hist` is cut at the test profile. Without `--quantity` a CLI quantity with
//!   no `zstd` on `PATH` is skipped with its reason. The binary passes the checkout it was compiled in, so the
//!   night's readiness smoke reads the owner's notes only from a release build made in the main checkout (R-35 Q5).
//! - Every file but the records lies below the working directory ([`work_dir`]; R-34 B-1): the CLI's stage.
//!
//! Exit 0 done, 1 a run or arm failed or a record could not be written, 2 a usage error.

pub mod codec;
pub mod corpus;
pub mod dict;
pub mod frames;

use crate::arm::{Arm, ArmError, Ticker, ValueFn};
use crate::host::{CommandRunner, HostKind};
use crate::m1::driver::{self, Args, EXIT_DONE, EXIT_FAILED, EXIT_USAGE};
use crate::run::{Context, RunSpec, measure};
use crate::units::Unit;
use codec::{CliStage, Codec, CodecKind, Op, stored};
use corpus::{Input, split};
use frames::Frames;
use moirai_vfs::Meter;
use std::cell::RefCell;
use std::ffi::OsString;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
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

/// The scale of the `hist` frames: P03, P04 and the tails' thresholds.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Profile {
    /// The production column (P03 256, P04 1 MiB, P07 4 MiB).
    Production,
    /// The test profile's column, the smoke's.
    Test,
}

impl Profile {
    /// The parameters.
    pub fn params(self) -> crate::tail::Params {
        match self {
            Profile::Production => crate::tail::Params::PRODUCTION,
            Profile::Test => crate::tail::Params::TEST,
        }
    }
}

/// What the binary passes in: the seams of the run, the process spawner and three facts of the process.
pub struct System<'a, M, T, R> {
    /// The measurement seam.
    pub meter: &'a M,
    /// The timer.
    pub ticker: T,
    /// The host-snapshot command runner.
    pub runner: R,
    /// Runs this driver again with the given arguments and returns its exit code.
    pub spawn: &'a mut dyn FnMut(&[OsString]) -> Result<u8, String>,
    /// The scale of a recording run's `hist` frames; a smoke cuts them at the test profile.
    pub profile: Profile,
    /// The `zstd` CLI on `PATH` ([`codec::zstd_on_path`]), when there is one.
    pub zstd: Option<PathBuf>,
    /// The working directory, below which every file but the records lies; `None` is the process's own
    /// ([`work_dir`]).
    pub work: Option<PathBuf>,
    /// The checkout the binary was built in, where a smoke finds the real corpora ([`corpus::from_checkout`]);
    /// `None` makes the smoke synthetic.
    pub checkout: Option<PathBuf>,
}

/// The working directory: `given`, or the process's current one ([MP §9.7] "The working directory"; R-34 B-1).
pub fn work_dir(given: Option<PathBuf>) -> Result<PathBuf, String> {
    match given {
        Some(p) => Ok(p),
        None => std::env::current_dir().map_err(|e| format!("the working directory: {e}")),
    }
}

/// The usage text.
pub const USAGE: &str = "usage: m6 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
          [--quantity <name>] --commit <sha> --toolchain <text> --out <private>/measurements/6
       m6 --smoke [--quantity <name>] --commit <sha> --toolchain <text>
       m6 --help
A notes, plan or size.cli quantity reads its corpus from --out's private root and the repository above it; a hist
quantity takes any --out. A size.cli quantity needs a zstd executable on PATH.
";

/// The synthetic corpora's seed.
const SYNTHETIC_SEED: u64 = 6;

fn no_load(c: Corpus) -> Result<Frames, String> {
    Err(format!("the {} corpus is not loaded here", c.name()))
}

/// `--smoke` ([MP §9.7]): every arm of each requested quantity once, nothing recorded. The notes and the plan
/// sections are the build checkout's when present, else synthetic, and a line says which; `hist` is cut at the test
/// profile. Without `--quantity`, a CLI quantity with no `zstd` on `PATH` is skipped with its reason.
fn smoke<M, T: Ticker + Clone, R>(
    sys: &System<'_, M, T, R>,
    qs: &[Quantity],
    asked: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let checkout = sys.checkout.as_deref();
    let mut code = EXIT_DONE;
    let mut loaded = |c: Corpus, f: fn(Option<&Path>, u64) -> Result<corpus::Loaded, String>| {
        if !qs.iter().any(|q| q.corpus == c) {
            return None;
        }
        match f(checkout, SYNTHETIC_SEED) {
            Ok(l) => {
                let _ = writeln!(out, "{}", l.line(c.name()));
                Some(l.inputs)
            }
            Err(e) => {
                let _ = writeln!(err, "m6 smoke corpus {} FAILED: {e}", c.name());
                code = EXIT_FAILED;
                None
            }
        }
    };
    let notes = loaded(Corpus::Notes, corpus::smoke_notes);
    let plan = loaded(Corpus::Plan, corpus::smoke_plan);
    let load = |c: Corpus| match c {
        Corpus::Notes => notes
            .clone()
            .map(Frames::singles)
            .ok_or_else(|| "no notes".to_string()),
        Corpus::Plan => plan
            .clone()
            .map(Frames::singles)
            .ok_or_else(|| "no plan".to_string()),
        Corpus::Hist => frames::hist(Profile::Test.params()),
    };
    let seams = Seams {
        load: &load,
        train: &dict::train,
        codec: &RealCodecs,
    };
    for q in qs {
        if q.measure == Measure::CliSize && sys.zstd.is_none() && !asked {
            let _ = writeln!(
                out,
                "m6 smoke {} skipped (needs-hardware: no zstd on PATH)",
                q.name
            );
            continue;
        }
        let run = || -> Result<(u64, Unit), String> {
            let p = prepare(q, &seams)?;
            let cli = Cli {
                zstd: sys.zstd.clone(),
                work: work_dir(sys.work.clone())?,
            };
            let mut a = arm(q, &p, &seams, &cli, sys.ticker.clone())?;
            let v = a
                .begin_block(1)
                .and_then(|()| a.sample())
                .map_err(|e| format!("{}: {e}", a.name()))?;
            Ok((v, a.unit()))
        };
        match run() {
            Ok((v, unit)) => {
                let _ = writeln!(
                    out,
                    "m6 smoke {} {} ok {v} {}",
                    q.name,
                    q.arm_name(),
                    unit.as_str()
                );
            }
            Err(e) => {
                let _ = writeln!(err, "m6 smoke {} FAILED: {e}", q.name);
                code = EXIT_FAILED;
            }
        }
    }
    code
}

/// One recorded run of `q` in this process: its corpus (from `--out`'s private root for the notes and the plan,
/// generated for `hist`), [`prepare`], its arm, [`measure`], and the record written directly in `--out`.
fn run_one<M: Meter, T: Ticker + Clone, R: CommandRunner>(
    sys: System<'_, M, T, R>,
    a: &Args,
    q: &Quantity,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let fail = |err: &mut dyn Write, code: u8, e: String| {
        let _ = writeln!(err, "m6 {}: {e}", q.name);
        code
    };
    let condition = match driver::condition(a) {
        Ok(c) => c,
        Err((code, e)) => return fail(err, code, e),
    };
    let dir = a.out.clone().unwrap_or_default();
    if q.measure == Measure::CliSize && sys.zstd.is_none() {
        return fail(
            err,
            EXIT_FAILED,
            format!("{} needs a zstd executable on PATH", q.name),
        );
    }
    let places = match q.corpus {
        Corpus::Hist => None,
        _ => match corpus::from_out(&dir) {
            Ok(p) => Some(p),
            Err(e) => return fail(err, EXIT_FAILED, e),
        },
    };
    let profile = sys.profile;
    let load = |c: Corpus| match (c, &places) {
        (Corpus::Hist, _) => frames::hist(profile.params()),
        (Corpus::Notes, Some(p)) => corpus::load_notes(&p.manifest).map(Frames::singles),
        (Corpus::Plan, Some(p)) => corpus::load_plan(&p.docs).map(Frames::singles),
        (c, None) => no_load(c),
    };
    let seams = Seams {
        load: &load,
        train: &dict::train,
        codec: &RealCodecs,
    };
    let rec = {
        let p = match prepare(q, &seams) {
            Ok(p) => p,
            Err(e) => return fail(err, EXIT_FAILED, e),
        };
        let cli = match work_dir(sys.work.clone()) {
            Ok(work) => Cli {
                zstd: sys.zstd.clone(),
                work,
            },
            Err(e) => return fail(err, EXIT_FAILED, e),
        };
        let mut arm = match arm(q, &p, &seams, &cli, sys.ticker.clone()) {
            Ok(a) => a,
            Err(e) => return fail(err, EXIT_FAILED, e),
        };
        let spec = RunSpec {
            measurement: MEASUREMENT,
            quantity: q.name.clone(),
            condition,
            min_tier: None,
        };
        let mut ctx = Context {
            meter: sys.meter,
            ticker: sys.ticker.clone(),
            runner: sys.runner,
            host: a.host.unwrap_or(HostKind::Laptop),
            toolchain: a.toolchain.clone().unwrap_or_default(),
            commit: a.commit.clone().unwrap_or_default(),
        };
        let t0 = driver::now_ms();
        let r = measure(&mut ctx, &spec, &mut [&mut *arm]);
        let t1 = driver::now_ms();
        match r {
            Ok(mut rec) => {
                driver::set_load_verdict(a, &mut rec, t0, t1, err, &format!("m6 {}", q.name));
                rec
            }
            Err(e) => return fail(err, EXIT_FAILED, e.to_string()),
        }
    };
    match driver::write_record(&rec, &dir) {
        Ok(p) => {
            let _ = writeln!(out, "m6 {} recorded {}", q.name, p.display());
            EXIT_DONE
        }
        Err(e) => fail(
            err,
            EXIT_FAILED,
            format!("the record could not be written: {e}"),
        ),
    }
}

/// The driver's command line ([MP §9.7], shared with measurements 1 and 2 through [`driver`]): parses `args`
/// (without the program name), then runs the smoke, one quantity, or every quantity in a child process of its own;
/// returns the exit code.
pub fn cli<M: Meter, T: Ticker + Clone, R: CommandRunner>(
    args: &[OsString],
    sys: System<'_, M, T, R>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let every = quantities(true);
    let names: Vec<&str> = every.iter().map(|q| q.name.as_str()).collect();
    let a = match driver::parse(args, &names) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m6: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    if a.help {
        let _ = out.write_all(USAGE.as_bytes());
        return EXIT_DONE;
    }
    let asked = a
        .quantity
        .as_ref()
        .and_then(|n| every.iter().find(|q| &q.name == n));
    if a.smoke {
        let qs: Vec<Quantity> = match asked {
            Some(q) => vec![q.clone()],
            None => every.clone(),
        };
        return smoke(&sys, &qs, asked.is_some(), out, err);
    }
    if let Some(q) = asked {
        return run_one(sys, &a, q, out, err);
    }
    let qs = quantities(sys.zstd.is_some());
    let names: Vec<&str> = qs.iter().map(|q| q.name.as_str()).collect();
    driver::each_in_own_process(args, &names, sys.spawn, out, err, "m6")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::valid_name;
    use crate::m6::corpus::{in_training, synthetic_notes, synthetic_plan};
    use crate::tail::Params;
    use crate::testkit::{FakeTicker, scratch_dir};
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

    /// The test corpora: the synthetic notes and plan sections, and the `hist` frames at the test profile.
    fn corpus(c: Corpus) -> Result<Frames, String> {
        match c {
            Corpus::Notes => Ok(Frames::singles(synthetic_notes(21))),
            Corpus::Plan => Ok(Frames::singles(synthetic_plan(21))),
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
                Ok(Frames::singles(v))
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

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// `--commit` and `--toolchain`, which every mode but `--help` needs.
    const CT: [&str; 4] = ["--commit", "c0ffee", "--toolchain", "rustc 1.98.1"];

    fn no_spawn(_: &[OsString]) -> Result<u8, String> {
        Err("no child process in this test".into())
    }

    /// Runs the command line with the real ticker, no spawn, `zstd`, the working directory `work` and the build
    /// checkout `checkout`; returns (exit, stdout, stderr).
    fn run_cli(
        args: &[&str],
        zstd: Option<PathBuf>,
        work: &Path,
        checkout: Option<PathBuf>,
    ) -> (u8, String, String) {
        let meter = crate::testkit::FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(args),
            System {
                meter: &meter,
                ticker: crate::arm::MonoTicker::new(),
                runner: crate::host::tests::Canned::good(),
                spawn: &mut spawn,
                profile: Profile::Test,
                zstd,
                work: Some(work.to_path_buf()),
                checkout,
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn smoke_args(q: Option<&str>) -> Vec<&str> {
        let mut v = vec!["--smoke"];
        v.extend(CT);
        if let Some(q) = q {
            v.extend(["--quantity", q]);
        }
        v
    }

    /// A synthetic checkout: `private/codec-bodies/` with a manifest and 12 bodies, and `docs/` with two files.
    fn tree(root: &Path) {
        let bodies = root.join("private").join("codec-bodies");
        std::fs::create_dir_all(bodies.join("notes")).unwrap();
        let mut manifest = String::from("# synthetic\nbody\tbytes\tsha256\tsource\n");
        for (k, b) in synthetic_notes(77).into_iter().take(12).enumerate() {
            let rel = format!("notes/n{k:02}.md");
            std::fs::write(bodies.join("notes").join(format!("n{k:02}.md")), &b.bytes).unwrap();
            manifest.push_str(&format!("{rel}\t{}\t0\tsynthetic\n", b.bytes.len()));
        }
        std::fs::write(bodies.join("manifest.tsv"), manifest).unwrap();
        let docs = root.join("docs");
        std::fs::create_dir_all(docs.join("sub")).unwrap();
        for (rel, text) in crate::m6::corpus::synthetic_docs(77).into_iter().take(4) {
            let p = rel.split('/').fold(docs.clone(), |p, c| p.join(c));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }

    /// The raw total of the measured side of `inputs`.
    fn measured_raw(inputs: Vec<Input>) -> u64 {
        split(inputs)
            .unwrap()
            .measured
            .iter()
            .map(|i| i.bytes.len() as u64)
            .sum()
    }

    /// The one record in `dir`, read back.
    fn one_record(dir: &Path) -> crate::record::RunRecord {
        let files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        crate::record::RunRecord::from_json(&v).unwrap()
    }

    /// `--smoke` runs each arm of every quantity once on the synthetic corpora (no checkout) with the corpus lines
    /// first, and without `zstd` on `PATH` skips the CLI's two with their reason; `--quantity` runs that one alone.
    #[test]
    fn smoke_exercises_every_arm_once() {
        let work = scratch_dir("m6-smoke");
        let (code, out, err) = run_cli(&smoke_args(None), None, &work, None);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("m6 smoke corpus notes synthetic (no checkout) 41 inputs "));
        assert!(lines[1].starts_with("m6 smoke corpus plan synthetic (no checkout) "));
        let runs: Vec<&str> = lines[2..].to_vec();
        assert_eq!(runs.len(), 29, "{out}");
        for (l, q) in runs.iter().zip(quantities(true)) {
            if q.measure == Measure::CliSize {
                assert_eq!(
                    *l,
                    format!(
                        "m6 smoke {} skipped (needs-hardware: no zstd on PATH)",
                        q.name
                    )
                );
                continue;
            }
            let w: Vec<&str> = l.split(' ').collect();
            assert_eq!(
                (w[0], w[1], w[2], w[3], w[4]),
                ("m6", "smoke", q.name.as_str(), q.arm_name(), "ok"),
                "{l}"
            );
            let unit = if matches!(q.measure, Measure::Enc(_) | Measure::Dec(_)) {
                "ns"
            } else {
                "bytes"
            };
            assert_eq!(w[6], unit, "{l}");
            if w[3] != "enc" && w[3] != "dec" {
                assert!(w[5].parse::<u64>().unwrap() > 0, "{l}");
            }
        }
        let (code, out, err) = run_cli(&smoke_args(Some("enc.lz4d.plan")), None, &work, None);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].starts_with("m6 smoke corpus plan synthetic"));
        assert!(lines[1].starts_with("m6 smoke enc.lz4d.plan enc ok "));
        let _ = std::fs::remove_dir_all(&work);
    }

    /// `--smoke` records nothing: no file is left below the working directory and no line says "recorded".
    #[test]
    fn smoke_records_nothing() {
        let work = scratch_dir("m6-smoke-nothing");
        let (code, out, err) = run_cli(&smoke_args(None), None, &work, None);
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(!out.contains("recorded"));
        assert_eq!(
            std::fs::read_dir(&work).unwrap().count(),
            0,
            "nothing below the working directory"
        );
        let _ = std::fs::remove_dir_all(&work);
    }

    /// A `--quantity` run writes exactly one record directly in `--out`, which is created when missing: measurement 6,
    /// its one arm in bytes, the given commit and toolchain, and the process's own readings (`private_peak` is the
    /// meter's, read in this process).
    #[test]
    fn run_writes_one_valid_record() {
        let root = scratch_dir("m6-run");
        let out_dir = root.join("made").join("here");
        let o = out_dir.to_str().unwrap().to_string();
        let mut args = vec![
            "--quantity",
            "raw.hist",
            "--host",
            "laptop",
            "--condition",
            "idle",
            "--out",
            &o,
        ];
        args.extend(CT);
        let (code, out, err) = run_cli(&args, None, &root, None);
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(out.contains("m6 raw.hist recorded"), "{out}");
        let rec = one_record(&out_dir);
        assert_eq!((rec.measurement, rec.quantity.as_str()), (6, "raw.hist"));
        assert_eq!(rec.arms.len(), 1);
        assert_eq!(
            (rec.arms[0].name.as_str(), rec.arms[0].unit),
            ("raw", Unit::Bytes)
        );
        assert_eq!(
            (rec.commit.as_str(), rec.toolchain.as_str()),
            ("c0ffee", "rustc 1.98.1")
        );
        assert_eq!(rec.process.private_peak, Some(3_000_000));
        let raw: u64 = frames::hist(Params::TEST)
            .unwrap()
            .blocks
            .iter()
            .map(|b| b.bytes.len() as u64)
            .sum();
        assert_eq!(rec.arms[0].reps[0].summary.p50, raw);
        assert!(rec.valid(), "{:?}", rec.disqualifications());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Usage errors exit 2 with the reason and the usage: the shared contract's (`--commit` and `--toolchain`
    /// required, `--smoke` with `--out`, a recording run without `--out`, `--host` or `--condition`, `--scratch`, an
    /// unknown flag or quantity); `--help` exits 0 with the usage.
    #[test]
    fn cli_refuses_usage_errors() {
        let work = scratch_dir("m6-usage");
        let run = ["--host", "laptop", "--condition", "idle", "--out", "/o"];
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (run.to_vec(), "--commit and --toolchain"),
            (vec!["--smoke"], "--commit and --toolchain"),
            (
                [&CT[..], &["--smoke", "--out", "/o"]].concat(),
                "takes no --out",
            ),
            (
                [&CT[..], &["--host", "laptop", "--condition", "idle"]].concat(),
                "needs --out",
            ),
            (
                [&CT[..], &["--out", "/o"]].concat(),
                "needs --host and --condition",
            ),
            (
                [&CT[..], &run[..], &["--quantity", "size.brotli.notes"]].concat(),
                "unknown quantity",
            ),
            (
                [&CT[..], &run[..], &["--scratch", "s"]].concat(),
                "unknown argument '--scratch'",
            ),
            (
                [&CT[..], &run[..], &["--list"]].concat(),
                "unknown argument '--list'",
            ),
        ];
        for (args, why) in cases {
            let (code, _, err) = run_cli(&args, None, &work, None);
            assert_eq!(code, EXIT_USAGE, "{args:?}");
            assert!(err.contains(why), "{args:?}: {err}");
            assert!(err.contains("usage: m6"), "{err}");
        }
        let (code, out, _) = run_cli(&["--help"], None, &work, None);
        assert_eq!((code, out.as_str()), (EXIT_DONE, USAGE));
        let names: Vec<String> = quantities(true).into_iter().map(|q| q.name).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert!(driver::parse(&os(&[&CT[..], &run[..]].concat()), &refs).is_ok());
        let _ = std::fs::remove_dir_all(&work);
    }

    /// Without `--quantity` each quantity runs in a child of its own, in grid order, with the same arguments plus
    /// `--quantity <q>`: the 27, or the 29 when `zstd` is on `PATH`; the exit is the worst child's, and a child that
    /// cannot start counts as failed.
    #[test]
    fn cli_without_quantity_spawns_one_process_per_run() {
        let meter = crate::testkit::FakeMeter::new(8_000_000_000);
        let base = os(&[
            &CT[..],
            &["--host", "laptop", "--condition", "idle", "--out", "/o"],
        ]
        .concat());
        for (zstd, bad, expect) in [
            (None, None, EXIT_DONE),
            (Some(PathBuf::from("/bin/zstd")), None, EXIT_DONE),
            (None, Some(Ok(1)), EXIT_FAILED),
            (None, Some(Err(())), EXIT_FAILED),
        ] {
            let mut calls: Vec<Vec<OsString>> = Vec::new();
            let mut spawn = |a: &[OsString]| -> Result<u8, String> {
                calls.push(a.to_vec());
                match (calls.len(), bad) {
                    (5, Some(Ok(c))) => Ok(c),
                    (5, Some(Err(()))) => Err("cannot start".into()),
                    _ => Ok(0),
                }
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let cli_present = zstd.is_some();
            let code = cli(
                &base,
                System {
                    meter: &meter,
                    ticker: crate::arm::MonoTicker::new(),
                    runner: crate::host::tests::Canned::good(),
                    spawn: &mut spawn,
                    profile: Profile::Test,
                    zstd,
                    work: None,
                    checkout: None,
                },
                &mut out,
                &mut err,
            );
            assert_eq!(code, expect);
            let qs = quantities(cli_present);
            assert_eq!(calls.len(), qs.len());
            for (c, q) in calls.iter().zip(qs) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
        }
    }

    /// A requested `size.cli.*` quantity with no `zstd` on `PATH` fails, in a recording run and in a smoke, with the
    /// reason; a smoke of every quantity skips it instead (see the smoke test).
    #[test]
    fn a_cli_quantity_needs_zstd_on_path() {
        let root = scratch_dir("m6-cli-q");
        tree(&root);
        let o = root.join("private").join("measurements").join("6");
        let o = o.to_str().unwrap().to_string();
        let mut args = vec![
            "--quantity",
            "size.cli.notes",
            "--host",
            "laptop",
            "--condition",
            "idle",
            "--out",
            &o,
        ];
        args.extend(CT);
        let (code, _, err) = run_cli(&args, None, &root, None);
        assert_eq!(code, EXIT_FAILED);
        assert!(
            err.contains("size.cli.notes needs a zstd executable on PATH"),
            "{err}"
        );
        let (code, _, err) = run_cli(&smoke_args(Some("size.cli.plan")), None, &root, None);
        assert_eq!(code, EXIT_FAILED);
        assert!(
            err.contains("size.cli.plan needs a zstd executable on PATH"),
            "{err}"
        );
        assert!(
            !root.join("private").join("measurements").exists(),
            "no record"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The night's plan lists exactly the driver's quantities in run order (R-27 Q2, R-34 M-13): the 27 without the
    /// CLI, or the 29 once the orchestrator has inserted the CLI's two at readiness.
    #[test]
    fn grid_equals_the_plans_list() {
        let plan = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/measure-night.toml");
        let text = std::fs::read_to_string(plan).unwrap();
        let list = driver::plan_quantities(&text, MEASUREMENT).unwrap();
        let names = |cli| -> Vec<String> { quantities(cli).into_iter().map(|q| q.name).collect() };
        assert!(list == names(false) || list == names(true), "{list:?}");
        assert_eq!(list.len(), 27);
    }

    /// A recording run of a notes or plan quantity reads its corpus from `--out` = `<P>/measurements/6`: the tree's
    /// bodies and docs, measured side only; another `--out`, or a missing manifest, fails naming the path. A `hist`
    /// quantity takes any `--out`.
    #[test]
    fn recording_runs_need_their_corpora() {
        let root = scratch_dir("m6-corpora");
        tree(&root);
        let out_dir = root.join("private").join("measurements").join("6");
        let o = out_dir.to_str().unwrap().to_string();
        let record = |q: &str, o: &str| {
            let mut args = vec![
                "--quantity",
                q,
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--out",
                o,
            ];
            args.extend(CT);
            run_cli(&args, None, &root, None)
        };
        let places = corpus::from_checkout(&root);
        for (q, want) in [
            (
                "raw.notes",
                measured_raw(corpus::load_notes(&places.manifest).unwrap()),
            ),
            (
                "raw.plan",
                measured_raw(corpus::load_plan(&places.docs).unwrap()),
            ),
        ] {
            let (code, _, err) = record(q, &o);
            assert_eq!(code, EXIT_DONE, "{q}: {err}");
            let rec = one_record(&out_dir);
            assert_eq!(rec.quantity, q);
            assert_eq!(rec.arms[0].reps[0].summary.p50, want, "{q}");
            std::fs::remove_dir_all(&out_dir).unwrap();
        }
        let elsewhere = root.join("elsewhere");
        let e = elsewhere.to_str().unwrap().to_string();
        let (code, _, err) = record("raw.notes", &e);
        assert_eq!(code, EXIT_FAILED);
        assert!(
            err.contains(&e) && err.contains("is not <private>/measurements/6"),
            "{err}"
        );
        let (code, _, err) = record("raw.hist", &e);
        assert_eq!(code, EXIT_DONE, "{err}");
        assert_eq!(one_record(&elsewhere).quantity, "raw.hist");
        std::fs::remove_file(&places.manifest).unwrap();
        let (code, _, err) = record("size.lz4.notes", &o);
        assert_eq!(code, EXIT_FAILED);
        assert!(err.contains("manifest.tsv"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The smoke reads the build checkout's corpora when they are present, says so, and measures them: `raw.notes`
    /// and `raw.plan` are the tree's measured totals; over an empty checkout it says synthetic and measures those.
    #[test]
    fn smoke_reads_the_checkouts_corpora_when_present() {
        let root = scratch_dir("m6-smoke-tree");
        tree(&root);
        let places = corpus::from_checkout(&root);
        let value = |out: &str, q: &str| -> u64 {
            let l = out
                .lines()
                .find(|l| l.starts_with(&format!("m6 smoke {q} ")))
                .unwrap();
            l.split(' ').nth(5).unwrap().parse().unwrap()
        };
        let (code, out, err) = run_cli(
            &smoke_args(Some("raw.notes")),
            None,
            &root,
            Some(root.clone()),
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(
            out.starts_with(&format!(
                "m6 smoke corpus notes {} 12 inputs ",
                places.manifest.display()
            )),
            "{out}"
        );
        let notes = corpus::load_notes(&places.manifest).unwrap();
        assert_eq!(value(&out, "raw.notes"), measured_raw(notes));
        let (code, out, err) = run_cli(
            &smoke_args(Some("raw.plan")),
            None,
            &root,
            Some(root.clone()),
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(
            out.starts_with(&format!("m6 smoke corpus plan {} ", places.docs.display())),
            "{out}"
        );
        let plan = corpus::load_plan(&places.docs).unwrap();
        assert_eq!(value(&out, "raw.plan"), measured_raw(plan));
        let empty = scratch_dir("m6-smoke-empty");
        let (code, out, err) = run_cli(
            &smoke_args(Some("raw.notes")),
            None,
            &empty,
            Some(empty.clone()),
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert!(
            out.starts_with("m6 smoke corpus notes synthetic ("),
            "{out}"
        );
        assert_eq!(
            value(&out, "raw.notes"),
            measured_raw(synthetic_notes(SYNTHETIC_SEED))
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// The working directory defaults to the process's current one in the library, where the tests see it (R-34
    /// B-1), and a given one is kept.
    #[test]
    fn work_defaults_to_the_working_directory() {
        assert_eq!(work_dir(None).unwrap(), std::env::current_dir().unwrap());
        let p = PathBuf::from("/some/work/6");
        assert_eq!(work_dir(Some(p.clone())).unwrap(), p);
    }
}
