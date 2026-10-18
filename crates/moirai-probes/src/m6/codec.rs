//! Measurement 6's codecs (cut D-4): `lz4_flex` 0.14 block with and without the raw-content dictionary, `ruzstd` 0.9
//! at `Fastest` with its frame decoder, and the `zstd` CLI at level 1 with the same dictionary, option (3)'s proxy
//! (`90-harness-agnostic-design.md:959-968`).
//!
//! - **`lz4`, `lz4d`** ([`Lz4`]): `block::compress_into` and `compress_into_with_dict` into a buffer of
//!   `get_maximum_output_size(max input)` made in setup; `decompress_into` and `decompress_into_with_dict`. Safe code
//!   (`safe-encode`, `safe-decode`): the crate's README shows its unsafe paths about 17-34 % faster, so these speeds
//!   are the conservative bound. The 0.14 API keeps no prepared dictionary, so every `lz4d` encode hashes the
//!   dictionary again, and that is in its time.
//! - **`zstd`** ([`Zstd`]): one `FrameCompressor` at `Fastest` kept for the arm, given each input as its source and
//!   its reused output as the drain; one `FrameDecoder` with `decode_all`. Without the `hash` feature the encoder
//!   writes no content checksum: `hist` frames carry their own `raw_xxh3` ([F10 §4.3]).
//! - **The CLI** ([`CliStage`], [`zstd_on_path`]): looked up on `PATH` only. Its working files lie below the working
//!   directory, `<work>/m6-cli/` (a stale one is replaced): `dict.raw`, one file per input and a file list; `zstd -1
//!   -D dict.raw --no-check --single-thread -q --output-dir-flat out --filelist list.txt` writes one frame per input,
//!   `out/<i>.raw.zst`. The stage removes its directory when dropped. It is size-only: its time is a process spawn
//!   and file I/O.

use ruzstd::decoding::FrameDecoder;
use ruzstd::encoding::{CompressionLevel, FrameCompressor, MatchGeneratorDriver};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The in-process codecs.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum CodecKind {
    /// `lz4_flex` block, no dictionary (F10 candidate 1).
    Lz4,
    /// `lz4_flex` block with the raw-content dictionary as preceding history (F10 candidate 2).
    Lz4Dict,
    /// `ruzstd` at `Fastest`, no dictionary (F10 candidate 3).
    Zstd,
}

impl CodecKind {
    /// The name in a quantity: `lz4`, `lz4d`, `zstd`.
    pub fn name(self) -> &'static str {
        match self {
            CodecKind::Lz4 => "lz4",
            CodecKind::Lz4Dict => "lz4d",
            CodecKind::Zstd => "zstd",
        }
    }
}

/// One timed codec call.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Op {
    /// An encode.
    Encode,
    /// A decode.
    Decode,
}

/// One codec instance with its buffers. `'a` is the life of the inputs it encodes.
pub trait Codec<'a> {
    /// Its kind.
    fn kind(&self) -> CodecKind;
    /// Encodes `input`; the output lives in the codec until its next call.
    fn encode(&mut self, input: &'a [u8]) -> Result<&[u8], String>;
    /// Decodes `enc` into `out`, which holds exactly the raw length; returns the bytes written.
    fn decode(&mut self, enc: &[u8], out: &mut [u8]) -> Result<usize, String>;
    /// The teardown's check of one call: a decode wrote the raw length, an encode of a non-empty input produced output.
    fn check(&self, op: Op, raw_len: usize, out_len: usize) -> Result<(), String> {
        match op {
            Op::Decode if out_len != raw_len => Err(format!(
                "{} decoded {out_len} bytes of {raw_len}",
                self.kind().name()
            )),
            Op::Encode if out_len == 0 && raw_len > 0 => Err(format!(
                "{} encoded {raw_len} bytes to none",
                self.kind().name()
            )),
            _ => Ok(()),
        }
    }
}

/// `lz4_flex` block, with or without a raw-content dictionary.
pub struct Lz4 {
    out: Vec<u8>,
    dict: Option<Vec<u8>>,
}

impl Lz4 {
    /// Without a dictionary, for inputs of at most `max_input` bytes.
    pub fn new(max_input: usize) -> Lz4 {
        Lz4 {
            out: vec![0; lz4_flex::block::get_maximum_output_size(max_input)],
            dict: None,
        }
    }

    /// With `dict` as preceding history.
    pub fn with_dict(max_input: usize, dict: &[u8]) -> Lz4 {
        Lz4 {
            dict: Some(dict.to_vec()),
            ..Lz4::new(max_input)
        }
    }
}

impl<'a> Codec<'a> for Lz4 {
    fn kind(&self) -> CodecKind {
        if self.dict.is_some() {
            CodecKind::Lz4Dict
        } else {
            CodecKind::Lz4
        }
    }

    fn encode(&mut self, input: &'a [u8]) -> Result<&[u8], String> {
        let n = match &self.dict {
            Some(d) => lz4_flex::block::compress_into_with_dict(input, &mut self.out, d),
            None => lz4_flex::block::compress_into(input, &mut self.out),
        }
        .map_err(|e| e.to_string())?;
        Ok(&self.out[..n])
    }

    fn decode(&mut self, enc: &[u8], out: &mut [u8]) -> Result<usize, String> {
        match &self.dict {
            Some(d) => lz4_flex::block::decompress_into_with_dict(enc, out, d),
            None => lz4_flex::block::decompress_into(enc, out),
        }
        .map_err(|e| e.to_string())
    }
}

/// `ruzstd` at `Fastest`, and its frame decoder.
pub struct Zstd<'a> {
    enc: FrameCompressor<&'a [u8], Vec<u8>, MatchGeneratorDriver>,
    dec: FrameDecoder,
    out: Vec<u8>,
}

impl Zstd<'_> {
    /// The encoder, the decoder and an output of `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Zstd {
            enc: FrameCompressor::new(CompressionLevel::Fastest),
            dec: FrameDecoder::new(),
            out: Vec::with_capacity(capacity),
        }
    }
}

impl<'a> Codec<'a> for Zstd<'a> {
    fn kind(&self) -> CodecKind {
        CodecKind::Zstd
    }

    fn encode(&mut self, input: &'a [u8]) -> Result<&[u8], String> {
        let mut drain = std::mem::take(&mut self.out);
        drain.clear();
        self.enc.set_source(input);
        self.enc.set_drain(drain);
        self.enc.compress();
        self.enc.take_source();
        self.out = self.enc.take_drain().ok_or("zstd: the drain is gone")?;
        Ok(&self.out)
    }

    fn decode(&mut self, enc: &[u8], out: &mut [u8]) -> Result<usize, String> {
        self.dec.decode_all(enc, out).map_err(|e| e.to_string())
    }
}

/// A codec of `kind` for inputs of at most `max_input` bytes; `lz4d` takes `dict`.
pub fn make<'a>(kind: CodecKind, dict: &[u8], max_input: usize) -> Box<dyn Codec<'a> + 'a> {
    match kind {
        CodecKind::Lz4 => Box::new(Lz4::new(max_input)),
        CodecKind::Lz4Dict => Box::new(Lz4::with_dict(max_input, dict)),
        CodecKind::Zstd => Box::new(Zstd::new(lz4_flex::block::get_maximum_output_size(
            max_input,
        ))),
    }
}

/// The stored payload of one input ([F10 §3.4]): the codec's output, or the raw bytes when the
/// output is not shorter.
pub fn stored(raw: usize, encoded: usize) -> usize {
    encoded.min(raw)
}

/// The `zstd` CLI on `path` (a `PATH` value): the first `zstd` executable of its directories.
pub fn zstd_on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let name = format!("zstd{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path?)
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

/// The CLI proxy's directory below the working directory.
pub const CLI_DIR: &str = "m6-cli";

/// The CLI proxy's working files below `<work>/m6-cli/`: the dictionary, one file per input, and the file list; the
/// directory is removed when the stage is dropped.
pub struct CliStage {
    dir: PathBuf,
    n: usize,
}

fn io(p: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", p.display())
}

impl CliStage {
    /// Writes `<work>/m6-cli/` anew: `dict.raw`, `in/<i>.raw` for each input, `list.txt`, and an empty `out/`.
    pub fn new(work: &Path, dict: &[u8], inputs: &[&[u8]]) -> Result<CliStage, String> {
        let dir = work.join(CLI_DIR);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(io(&dir))?;
        }
        let stage = CliStage {
            dir,
            n: inputs.len(),
        };
        for sub in ["in", "out"] {
            let d = stage.dir.join(sub);
            std::fs::create_dir_all(&d).map_err(io(&d))?;
        }
        let d = stage.dir.join("dict.raw");
        std::fs::write(&d, dict).map_err(io(&d))?;
        let mut list = String::new();
        for (i, b) in inputs.iter().enumerate() {
            let p = stage.dir.join("in").join(format!("{i}.raw"));
            std::fs::write(&p, b).map_err(io(&p))?;
            list.push_str(&format!("in/{i}.raw\n"));
        }
        let l = stage.dir.join("list.txt");
        std::fs::write(&l, list).map_err(io(&l))?;
        Ok(stage)
    }

    /// The stage's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn run(&self, zstd: &Path, args: &[&str]) -> Result<(), String> {
        let o = std::process::Command::new(zstd)
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(|e| format!("{}: {e}", zstd.display()))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{} {}: {}; {}",
                zstd.display(),
                args.join(" "),
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            ))
        }
    }

    fn fresh(&self, sub: &str) -> Result<PathBuf, String> {
        let d = self.dir.join(sub);
        if d.exists() {
            std::fs::remove_dir_all(&d).map_err(io(&d))?;
        }
        std::fs::create_dir(&d).map_err(io(&d))?;
        Ok(d)
    }

    /// Compresses every input with the dictionary into an emptied `out/`; returns each output's length, in input
    /// order.
    pub fn compress(&self, zstd: &Path) -> Result<Vec<u64>, String> {
        let out = self.fresh("out")?;
        self.run(
            zstd,
            &[
                "-1",
                "-D",
                "dict.raw",
                "--no-check",
                "--single-thread",
                "-q",
                "--output-dir-flat",
                "out",
                "--filelist",
                "list.txt",
            ],
        )?;
        (0..self.n)
            .map(|i| {
                let p = out.join(format!("{i}.raw.zst"));
                std::fs::metadata(&p).map(|m| m.len()).map_err(io(&p))
            })
            .collect()
    }

    /// Decodes every `out/<i>.raw.zst`, with the dictionary or without, into an emptied `dec/`; returns the decoded
    /// inputs in order.
    pub fn decompress(&self, zstd: &Path, with_dict: bool) -> Result<Vec<Vec<u8>>, String> {
        let dec = self.fresh("dec")?;
        let mut zlist = String::new();
        for i in 0..self.n {
            zlist.push_str(&format!("out/{i}.raw.zst\n"));
        }
        let l = self.dir.join("zlist.txt");
        std::fs::write(&l, zlist).map_err(io(&l))?;
        let mut args = vec!["-d"];
        if with_dict {
            args.extend(["-D", "dict.raw"]);
        }
        args.extend(["-q", "--output-dir-flat", "dec", "--filelist", "zlist.txt"]);
        self.run(zstd, &args)?;
        (0..self.n)
            .map(|i| {
                let p = dec.join(format!("{i}.raw"));
                std::fs::read(&p).map_err(io(&p))
            })
            .collect()
    }
}

impl Drop for CliStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m6::corpus::{Input, split, synthetic_notes};
    use crate::m6::dict::train;
    use crate::m6::frames::hist;
    use crate::tail::Params;
    use crate::testkit::scratch_dir;

    type Bytes = Vec<Vec<u8>>;

    /// The synthetic notes' training inputs and measured bodies, and the `hist` blocks at the test profile.
    fn corpora() -> (Vec<Input>, Bytes, Bytes) {
        let s = split(synthetic_notes(11)).unwrap();
        let blocks = hist(Params::TEST).unwrap().blocks;
        let b = |v: Vec<Input>| v.into_iter().map(|i| i.bytes).collect();
        (s.training, b(s.measured), b(blocks))
    }

    /// Encodes and decodes every input with `c`; returns the encoded inputs.
    fn round_trip<'a>(c: &mut dyn Codec<'a>, inputs: &'a [Vec<u8>]) -> Vec<Vec<u8>> {
        inputs
            .iter()
            .map(|raw| {
                let enc = c.encode(raw).unwrap().to_vec();
                c.check(Op::Encode, raw.len(), enc.len()).unwrap();
                let mut out = vec![0u8; raw.len()];
                let n = c.decode(&enc, &mut out).unwrap();
                c.check(Op::Decode, raw.len(), n).unwrap();
                assert!(out == *raw, "{} round trip", c.kind().name());
                enc
            })
            .collect()
    }

    /// `lz4` round-trips every synthetic body and every `hist` block, and shrinks the bodies.
    #[test]
    fn lz4_round_trips_every_body_and_frame() {
        let (_, bodies, blocks) = corpora();
        let max = bodies.iter().chain(&blocks).map(Vec::len).max().unwrap();
        let mut c = Lz4::new(max);
        assert_eq!(Codec::kind(&c), CodecKind::Lz4);
        let enc = round_trip(&mut c, &bodies);
        let (raw, packed): (usize, usize) = (
            bodies.iter().map(Vec::len).sum(),
            enc.iter().map(Vec::len).sum(),
        );
        assert!(packed < raw, "{packed} >= {raw}");
        round_trip(&mut c, &blocks);
        assert!(c.check(Op::Decode, 10, 9).is_err());
        assert!(c.check(Op::Encode, 10, 0).is_err());
    }

    /// `lz4d` round-trips every body with the dictionary trained on the training split, and needs it: decoding its
    /// output without the dictionary fails or differs on some body, and encoding with it is shorter than without.
    #[test]
    fn lz4_dict_round_trips_and_needs_its_dictionary() {
        let (training, bodies, blocks) = corpora();
        let dict = train(&training);
        let max = bodies.iter().chain(&blocks).map(Vec::len).max().unwrap();
        let mut c = Lz4::with_dict(max, &dict);
        assert_eq!(Codec::kind(&c), CodecKind::Lz4Dict);
        let enc = round_trip(&mut c, &bodies);
        round_trip(&mut c, &blocks);
        let mut plain = Lz4::new(max);
        let needs = bodies.iter().zip(&enc).any(|(raw, e)| {
            let mut out = vec![0u8; raw.len()];
            !matches!(plain.decode(e, &mut out), Ok(n) if n == raw.len() && out == *raw)
        });
        assert!(needs, "every lz4d body decodes without its dictionary");
        let without: usize = round_trip(&mut plain, &bodies).iter().map(Vec::len).sum();
        let with: usize = enc.iter().map(Vec::len).sum();
        assert!(with < without, "{with} >= {without}");
    }

    /// `zstd` (ruzstd `Fastest`) round-trips every body and every block with one kept encoder and decoder, and its
    /// output is one frame that the decoder accepts whole.
    #[test]
    fn zstd_round_trips_every_body_and_frame() {
        let (_, bodies, blocks) = corpora();
        let mut c = Zstd::new(1 << 16);
        assert_eq!(Codec::kind(&c), CodecKind::Zstd);
        let enc = round_trip(&mut c, &bodies);
        let (raw, packed): (usize, usize) = (
            bodies.iter().map(Vec::len).sum(),
            enc.iter().map(Vec::len).sum(),
        );
        assert!(packed < raw, "{packed} >= {raw}");
        assert!(
            enc.iter().all(|e| e.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])),
            "a zstd frame"
        );
        round_trip(&mut c, &blocks);
        let mut boxed = make(CodecKind::Zstd, &[], 1 << 16);
        round_trip(boxed.as_mut(), &bodies);
        assert_eq!(stored(10, 12), 10);
        assert_eq!(stored(10, 7), 7);
    }

    /// The CLI at level 1 with the raw-content dictionary round-trips every measured body, one frame per input, and
    /// needs the dictionary: decoding without it fails or differs. Run with `--ignored` and the CLI's directory on
    /// `PATH` (`D:\moirai-tools\zstd\zstd-v1.5.7-win64`).
    #[test]
    #[ignore = "needs-hardware: the zstd CLI on PATH (docs/m0/tools.md:354)"]
    fn zstd_cli_round_trips_with_the_dictionary() {
        let zstd = zstd_on_path(std::env::var_os("PATH").as_deref())
            .expect("no zstd on PATH: prefix it with the CLI's directory (docs/m0/tools.md:354)");
        let (training, bodies, _) = corpora();
        let dict = train(&training);
        let work = scratch_dir("m6-cli");
        let refs: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();
        let stage = CliStage::new(&work, &dict, &refs).unwrap();
        let sizes = stage.compress(&zstd).unwrap();
        assert_eq!(sizes.len(), bodies.len());
        let packed: u64 = sizes.iter().sum();
        let raw: u64 = bodies.iter().map(|b| b.len() as u64).sum();
        assert!(packed < raw, "{packed} >= {raw}");
        assert_eq!(stage.decompress(&zstd, true).unwrap(), bodies);
        let without = stage.decompress(&zstd, false);
        assert!(
            !matches!(&without, Ok(d) if *d == bodies),
            "every frame decodes without the dictionary"
        );
        drop(stage);
        assert!(
            !work.join(CLI_DIR).exists(),
            "the stage removes its directory"
        );
        let _ = std::fs::remove_dir_all(&work);
    }

    /// The CLI is looked up on `PATH` only: the first directory that holds a `zstd` file; a directory named like it
    /// is skipped, and no `PATH` finds none.
    #[test]
    fn zstd_cli_is_looked_up_on_path() {
        let root = scratch_dir("m6-path");
        let name = format!("zstd{}", std::env::consts::EXE_SUFFIX);
        let (empty, fake, shadow) = (root.join("empty"), root.join("fake"), root.join("shadow"));
        for d in [&empty, &fake, &shadow] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(fake.join(&name), b"not a real zstd").unwrap();
        std::fs::create_dir(shadow.join(&name)).unwrap();
        let path = std::env::join_paths([&empty, &shadow, &fake]).unwrap();
        assert_eq!(zstd_on_path(Some(path.as_os_str())), Some(fake.join(&name)));
        let path = std::env::join_paths([&empty, &shadow]).unwrap();
        assert_eq!(zstd_on_path(Some(path.as_os_str())), None);
        assert_eq!(zstd_on_path(None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The stage writes `<work>/m6-cli/` anew over a stale directory: exactly `dict.raw`, `in/<i>.raw`, `list.txt`
    /// and an empty `out/`, and nothing else; dropped, it removes the directory.
    #[test]
    fn zstd_cli_staging_replaces_a_stale_directory() {
        let work = scratch_dir("m6-stage");
        let listing = |d: &Path| -> Vec<String> {
            let mut v = Vec::new();
            for e in walk(d) {
                v.push(
                    e.strip_prefix(d)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
            v.sort();
            v
        };
        let plant = || {
            let d = work.join(CLI_DIR).join("out");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("old.raw.zst"), b"stale").unwrap();
            std::fs::write(work.join(CLI_DIR).join("stale.txt"), b"stale").unwrap();
        };
        plant();
        let s = CliStage::new(
            &work,
            b"dictionary",
            &[&b"one"[..], &b"two"[..], &b"three"[..]],
        )
        .unwrap();
        assert_eq!(s.dir(), work.join(CLI_DIR));
        assert_eq!(
            listing(s.dir()),
            [
                "dict.raw", "in", "in/0.raw", "in/1.raw", "in/2.raw", "list.txt", "out"
            ]
        );
        assert_eq!(
            std::fs::read(s.dir().join("in").join("1.raw")).unwrap(),
            b"two"
        );
        assert_eq!(
            std::fs::read_to_string(s.dir().join("list.txt")).unwrap(),
            "in/0.raw\nin/1.raw\nin/2.raw\n"
        );
        drop(s);
        assert!(!work.join(CLI_DIR).exists());
        plant();
        let s = CliStage::new(&work, b"d2", &[&b"only"[..]]).unwrap();
        assert_eq!(
            listing(s.dir()),
            ["dict.raw", "in", "in/0.raw", "list.txt", "out"]
        );
        assert_eq!(std::fs::read(s.dir().join("dict.raw")).unwrap(), b"d2");
        drop(s);
        let _ = std::fs::remove_dir_all(&work);
    }

    /// Every path below `d`, directories included.
    fn walk(d: &Path) -> Vec<PathBuf> {
        let mut v = Vec::new();
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                v.extend(walk(&p));
            }
            v.push(p);
        }
        v
    }
}
