//! `xtask hex`: the generic fixture assembler of WP-20 (docs/m0/PLAN.md §3.2 item 1, §2.4 `xxhash-rust`;
//! docs/m0/authors.md §6 item 3). It turns a hand-written `.hex` file into bytes. It knows no moirai structure (S1):
//! it assembles bytes, labels and a few directives that compute hashes, lengths and integers over label ranges and
//! literal bytes, so a fixture author writes every field from the specification text and lets the tool fill in the
//! values no person can compute by hand.
//!
//! The text format and the command line are [`USAGE`]'s. Encodings follow [F01]: integers are little-endian
//! ([F01 §4.1], [F01 §5.1]), `uvar` is unsigned LEB128 in its shortest form and `svar` is zigzag then `uvar`
//! ([F01 §5.2], [F01 §5.3]); an XXH3-64 value is stored as a little-endian `u64` and an XXH3-128 value as its low and
//! then its high 64 bits, each little-endian ([F01 §7.2]); BLAKE3-128 is the first 16 bytes of BLAKE3-256
//! ([F01 §7.1]). WP-20 names `{xxh3_64 a..b}`, `{blake3_256 a..b}` and `{len a..b}`; the other directives cover the
//! rest of [F01 §7.1]'s hash set that the fixtures need (the `HEAD` slot's XXH3-128, [F04 §3]; the seeded chain
//! trailer, [F05 §4.3]; a record checksum over two disjoint ranges, [F05 §3.4]). Literal operands (strings and hex
//! bytes) cover the derivations whose input is not all stored bytes: a constant domain prefix framed with `lp()`
//! ([F01 §6.3]) in front of stored fields ([F01 §7.1], [F01 §7.3]).
//!
//! Assembly runs in three passes: parse (every label reference must name a label defined once), layout (item sizes
//! and label offsets, repeated until they settle, because a `uvar` length or a padding may depend on a label further
//! on), and evaluation (fixed bytes first, then each hash once the bytes it reads are final, in a topological order
//! of the hashes' reads; a hash that reads its own output, directly or through others, is refused, as [F01 §7.4]
//! requires of every checksum). A hash streams its operands into the hasher, so the only buffers besides the output
//! are the few bytes of nested directives.
//!
//! `--check` re-assembles every `fixtures/hex/**.hex` and compares it with the `.bin` committed beside it, which every
//! `.hex` must have, and with its `!expect` lines; the gate runs it as the `hex` step. The committed `.bin` is the
//! fixture's interface: the format oracle (WP-95), which has no workspace dependency, and the M1 codec read the
//! bytes from disk (authors.md §6 item 3).

use crate::diag::Diag;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The command line and the `.hex` format, printed by `cargo xtask hex --help` and on a usage error.
pub const USAGE: &str = "usage:
  cargo xtask hex <file.hex>... [-o <out.bin>]
      assemble each file into <file>.bin beside it (-o: a single input only); print its length and BLAKE3-256; a
      file that does not assemble, or whose !expect lines disagree, is reported and not written, the others are
      written; exit 1 if any file failed
  cargo xtask hex --digest <file.hex>...
      print each file's length and BLAKE3-256, for an `!expect` line; write nothing
  cargo xtask hex --check [<file or directory>...]
      re-assemble every .hex under the paths (default fixtures/hex) and compare it with the committed .bin beside it
      (same name, extension .bin), which every .hex needs, and with its `!expect` lines; a .bin with no .hex is a
      finding too; exit 1 on any finding (the gate's `hex` step). The committed .bin is what the format oracle and
      the codec read; `!expect` lines are an extra pin

.hex format (UTF-8; LF or CRLF line ends; one leading BOM skipped):
  # comment                   to the end of the line (not inside a string)
  4d 4f 49 52   4D4F4952      hex bytes: pairs of hex digits, either case, spaces optional
  00*4080                     repetition: the token's bytes n times (n decimal or 0x hex)
  \"MOIR\"                      a string: its UTF-8 bytes; escapes \\\\ \\\" \\n \\r \\t \\0 \\xNN
  name:                       a label at the current offset: [A-Za-z_][A-Za-z0-9_.]*, without \"..\" and not ending
                              in \".\" (so a..b stays a range), defined once per file
  {directive ...}             one of the directives below, on one line
  !expect blake3_256 <hex>    a pragma line: the BLAKE3-256 (64 hex digits) of the assembled file
  !expect len <n>             a pragma line: the assembled file's length in bytes

positions and ranges:
  a position is a label, label+n, label-n, or an offset n from the start of the file (decimal or 0x hex);
  a range a..b is the output bytes [a, b)

directives (an operand r is a range a..b, a nested {directive}, a \"string\" or a hex-bytes token such as 0e000000,
standing for its bytes; several are concatenated in order; a literal operand is not part of the output):
  {xxh3_64 r... [seed=s]}     8 bytes: XXH3-64 as a little-endian u64 (F01 §7.1, §7.2); seed s: an integer
                              (decimal or 0x hex), or a range, nested directive or string of exactly 8 bytes read as
                              a little-endian u64; default 0
  {xxh3_128 r... [seed=s]}    16 bytes: XXH3-128, low 64 bits then high 64 bits, each little-endian
  {blake3_256 r...}           32 bytes: BLAKE3 in its default mode
  {blake3_128 r...}           16 bytes: the first 16 bytes of BLAKE3
  {len r... [type]}           the operands' total length as type u8, u16, u32 (default), u64 or uvar
  {u8 n} {u16 n} {u32 n} {u64 n} {i8 n} {i16 n} {i32 n} {i64 n}
                              an integer, little-endian (two's complement for i*)
  {uvar n} {svar n}           unsigned LEB128, shortest form; zigzag then LEB128 (F01 §5.2, §5.3)
  {align n [xx]}              fill byte xx (default 00) up to the next offset that is a multiple of n
  {pad_to p [xx]}             fill byte xx (default 00) up to position p; refused when the output is already past p

  Example, a derivation over an lp()-framed domain prefix and stored bytes (F01 §6.3, §7.1):
    {blake3_128 {len \"moirai-file-v1\"} \"moirai-file-v1\" f..g}

A hash may not read its own bytes, directly or through other hashes (F01 §7.4). The assembler knows no moirai
structure: every field is written from the specification text.";

/// The largest file the assembler builds: fixtures are hand-written, and a repetition typo must not exhaust RAM.
const MAX_OUTPUT: u64 = 64 << 20;
/// Layout rounds before the sizes must have settled.
const MAX_ROUNDS: usize = 256;

// ---------------------------------------------------------------------------------------------------------------
// The parsed form

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    Uvar,
    Svar,
}

impl IntType {
    fn parse(s: &str) -> Option<IntType> {
        Some(match s {
            "u8" => IntType::U8,
            "u16" => IntType::U16,
            "u32" => IntType::U32,
            "u64" => IntType::U64,
            "i8" => IntType::I8,
            "i16" => IntType::I16,
            "i32" => IntType::I32,
            "i64" => IntType::I64,
            "uvar" => IntType::Uvar,
            "svar" => IntType::Svar,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            IntType::U8 => "u8",
            IntType::U16 => "u16",
            IntType::U32 => "u32",
            IntType::U64 => "u64",
            IntType::I8 => "i8",
            IntType::I16 => "i16",
            IntType::I32 => "i32",
            IntType::I64 => "i64",
            IntType::Uvar => "uvar",
            IntType::Svar => "svar",
        }
    }

    fn unsigned(self) -> bool {
        matches!(
            self,
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::U64 | IntType::Uvar
        )
    }

    /// The encoding of `v`, refused when `v` does not fit the type.
    fn encode(self, v: i128) -> Result<Vec<u8>, String> {
        let (lo, hi): (i128, i128) = match self {
            IntType::U8 => (0, u8::MAX.into()),
            IntType::U16 => (0, u16::MAX.into()),
            IntType::U32 => (0, u32::MAX.into()),
            IntType::U64 | IntType::Uvar => (0, u64::MAX.into()),
            IntType::I8 => (i8::MIN.into(), i8::MAX.into()),
            IntType::I16 => (i16::MIN.into(), i16::MAX.into()),
            IntType::I32 => (i32::MIN.into(), i32::MAX.into()),
            IntType::I64 | IntType::Svar => (i64::MIN.into(), i64::MAX.into()),
        };
        if v < lo || v > hi {
            return Err(format!("{v} does not fit {}", self.name()));
        }
        let le = v.to_le_bytes();
        Ok(match self {
            IntType::U8 | IntType::I8 => le[..1].to_vec(),
            IntType::U16 | IntType::I16 => le[..2].to_vec(),
            IntType::U32 | IntType::I32 => le[..4].to_vec(),
            IntType::U64 | IntType::I64 => le[..8].to_vec(),
            // In range, so the casts are exact.
            IntType::Uvar => uvar(v as u64),
            IntType::Svar => {
                let i = v as i64;
                uvar(((i << 1) ^ (i >> 63)) as u64)
            }
        })
    }
}

/// Unsigned LEB128 in its shortest form ([F01 §5.2]).
fn uvar(mut v: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

fn uvar_len(v: u64) -> u64 {
    u64::from((64 - v.leading_zeros()).max(1).div_ceil(7))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashKind {
    Xxh3_64,
    Xxh3_128,
    Blake3_256,
    Blake3_128,
}

impl HashKind {
    fn parse(s: &str) -> Option<HashKind> {
        Some(match s {
            "xxh3_64" => HashKind::Xxh3_64,
            "xxh3_128" => HashKind::Xxh3_128,
            "blake3_256" => HashKind::Blake3_256,
            "blake3_128" => HashKind::Blake3_128,
            _ => return None,
        })
    }

    fn width(self) -> u64 {
        match self {
            HashKind::Xxh3_64 => 8,
            HashKind::Xxh3_128 | HashKind::Blake3_128 => 16,
            HashKind::Blake3_256 => 32,
        }
    }

    fn seeded(self) -> bool {
        matches!(self, HashKind::Xxh3_64 | HashKind::Xxh3_128)
    }

    /// The digest of the bytes `feed` passes to its sink, streamed into the hasher. XXH3 with seed 0 is XXH3 without
    /// a seed.
    fn digest(
        self,
        seed: u64,
        feed: impl FnOnce(&mut dyn FnMut(&[u8])) -> Result<(), String>,
    ) -> Result<Vec<u8>, String> {
        Ok(match self {
            HashKind::Xxh3_64 | HashKind::Xxh3_128 => {
                let mut h = xxhash_rust::xxh3::Xxh3::with_seed(seed);
                feed(&mut |b| h.update(b))?;
                if self == HashKind::Xxh3_64 {
                    h.digest().to_le_bytes().to_vec()
                } else {
                    // u128 little-endian is low64 LE followed by high64 LE ([F01 §7.2]).
                    h.digest128().to_le_bytes().to_vec()
                }
            }
            HashKind::Blake3_256 | HashKind::Blake3_128 => {
                let mut h = blake3::Hasher::new();
                feed(&mut |b| {
                    h.update(b);
                })?;
                let width = if self == HashKind::Blake3_256 { 32 } else { 16 };
                h.finalize().as_bytes()[..width].to_vec()
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Base {
    Abs(u64),
    Label(String),
}

/// A position: a label or an absolute offset, moved by `delta`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pos {
    base: Base,
    delta: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operand {
    Range(Pos, Pos),
    Nested(Box<Dir>),
    /// A string or hex-bytes token: bytes that are not in the output (a domain prefix, a constant).
    Literal(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seed {
    Value(u64),
    /// An operand of exactly 8 bytes, read as a little-endian `u64`.
    Bytes(Operand),
}

/// A directive that yields bytes (top level or nested).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Dir {
    Hash {
        kind: HashKind,
        ops: Vec<Operand>,
        seed: Option<Seed>,
    },
    Len {
        ops: Vec<Operand>,
        ty: IntType,
    },
    /// An integer directive, encoded when parsed.
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// `bytes` repeated `count` times.
    Bytes {
        bytes: Vec<u8>,
        count: u64,
    },
    Label(String),
    Dir(Dir),
    Align {
        n: u64,
        fill: u8,
    },
    PadTo {
        pos: Pos,
        fill: u8,
    },
}

#[derive(Clone, Debug)]
struct Item {
    kind: Kind,
    line: u32,
}

/// An `!expect` pragma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    Blake3([u8; 32]),
    Len(u64),
}

/// An assembled file and its `!expect` pragmas with their lines.
#[derive(Debug)]
pub struct Assembled {
    pub bytes: Vec<u8>,
    pub expects: Vec<(Expect, u32)>,
}

impl Assembled {
    /// What the `!expect` lines say that the bytes are not.
    pub fn mismatches(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (e, line) in &self.expects {
            match e {
                Expect::Blake3(h) => {
                    let got = blake3::hash(&self.bytes);
                    if got.as_bytes() != h {
                        out.push(format!(
                            "line {line}: !expect blake3_256 {}, assembled {}",
                            hex_str(h),
                            hex_str(got.as_bytes())
                        ));
                    }
                }
                Expect::Len(n) => {
                    if *n != self.bytes.len() as u64 {
                        out.push(format!(
                            "line {line}: !expect len {n}, assembled {}",
                            self.bytes.len()
                        ));
                    }
                }
            }
        }
        out
    }
}

pub fn hex_str(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Parsing

fn at(line: u32, m: impl std::fmt::Display) -> String {
    format!("line {line}: {m}")
}

fn parse_u64(s: &str) -> Option<u64> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) if !h.is_empty() && h.bytes().all(|c| c.is_ascii_hexdigit()) => {
            u64::from_str_radix(h, 16).ok()
        }
        Some(_) => None,
        None if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) => s.parse().ok(),
        None => None,
    }
}

fn parse_int(s: &str) -> Option<i128> {
    match s.strip_prefix('-') {
        Some(rest) => parse_u64(rest).map(|v| -i128::from(v)),
        None => parse_u64(s).map(i128::from),
    }
}

fn valid_label(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'.')
        && !s.contains("..")
        && !s.ends_with('.')
}

fn hex_bytes(tok: &str) -> Result<Vec<u8>, String> {
    if !tok.len().is_multiple_of(2) {
        return Err(format!(
            "'{tok}': an odd number of hex digits (bytes are pairs of digits)"
        ));
    }
    tok.as_bytes()
        .chunks(2)
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .filter(|d| d.bytes().all(|c| c.is_ascii_hexdigit()))
                .and_then(|d| u8::from_str_radix(d, 16).ok())
                .ok_or_else(|| {
                    format!("'{tok}' is not hex bytes, a label (name:) or a repetition (xx*n)")
                })
        })
        .collect()
}

fn parse_pos(s: &str) -> Result<Pos, String> {
    let split = s
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '+' || *c == '-')
        .map(|(i, _)| i);
    let (base, delta) = match split {
        Some(i) => {
            let n = parse_u64(&s[i + 1..])
                .and_then(|n| i64::try_from(n).ok())
                .ok_or_else(|| {
                    format!("'{s}': the offset after '{}' is not a number", &s[i..=i])
                })?;
            (&s[..i], if &s[i..=i] == "-" { -n } else { n })
        }
        None => (s, 0),
    };
    let base = if base.starts_with(|c: char| c.is_ascii_digit()) {
        Base::Abs(parse_u64(base).ok_or_else(|| format!("'{base}' is not an offset"))?)
    } else if valid_label(base) {
        Base::Label(base.to_string())
    } else {
        return Err(format!(
            "'{s}' is not a position (a label, label±n or an offset)"
        ));
    };
    Ok(Pos { base, delta })
}

fn parse_range(s: &str) -> Result<(Pos, Pos), String> {
    let (a, b) = s
        .split_once("..")
        .ok_or_else(|| format!("'{s}' is not a range a..b"))?;
    Ok((parse_pos(a)?, parse_pos(b)?))
}

/// Tracks whether a scan is inside a string literal, so braces, white space and `#` there are text.
#[derive(Default)]
struct StrState {
    inside: bool,
    escaped: bool,
}

impl StrState {
    /// Advances over `c`; true when `c` belongs to a string literal (its quotes included).
    fn step(&mut self, c: char) -> bool {
        if self.inside {
            if self.escaped {
                self.escaped = false;
            } else if c == '\\' {
                self.escaped = true;
            } else if c == '"' {
                self.inside = false;
            }
            true
        } else if c == '"' {
            self.inside = true;
            true
        } else {
            false
        }
    }
}

/// Splits a directive's inside into tokens at white space outside nested braces and strings.
fn dir_tokens(inner: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let mut s = StrState::default();
    for (i, c) in inner.char_indices() {
        if s.step(c) {
            start.get_or_insert(i);
            continue;
        }
        match c {
            '{' => {
                depth += 1;
                start.get_or_insert(i);
            }
            '}' => {
                depth = depth.checked_sub(1).ok_or("a '}' without its '{'")?;
            }
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    out.push(&inner[s..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if s.inside {
        return Err("a string without its closing '\"'".into());
    }
    if depth != 0 {
        return Err("a '{' without its '}'".into());
    }
    if let Some(s) = start {
        out.push(&inner[s..]);
    }
    Ok(out)
}

fn braced(tok: &str) -> Option<&str> {
    tok.strip_prefix('{').and_then(|t| t.strip_suffix('}'))
}

/// An operand token: a nested directive, a range, a string, or (with `hex`) a hex-bytes token. A seed takes no
/// hex-bytes token, because `seed=` followed by digits is an integer.
fn parse_operand(tok: &str, hex: bool) -> Result<Operand, String> {
    if let Some(inner) = braced(tok) {
        return match parse_dir(inner, false)? {
            DirOrLayout::Dir(d) => Ok(Operand::Nested(Box::new(d))),
            DirOrLayout::Layout(_) => Err(format!("'{tok}': align and pad_to cannot be nested")),
        };
    }
    if tok.starts_with('"') {
        let (bytes, n) = parse_string(tok)?;
        if n != tok.len() {
            return Err(format!(
                "'{tok}': text after the string's closing quote (operands are separated by spaces)"
            ));
        }
        return Ok(Operand::Literal(bytes));
    }
    if tok.contains("..") {
        let (a, b) = parse_range(tok)?;
        return Ok(Operand::Range(a, b));
    }
    if hex && !tok.is_empty() && tok.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Operand::Literal(hex_bytes(tok)?));
    }
    Err(format!(
        "'{tok}': an operand is a range a..b, a nested {{directive}}, a \"string\"{}",
        if hex { " or hex bytes" } else { "" }
    ))
}

enum DirOrLayout {
    Dir(Dir),
    Layout(Kind),
}

fn fill_byte(tok: Option<&&str>, name: &str) -> Result<u8, String> {
    match tok {
        None => Ok(0),
        Some(t) => match hex_bytes(t) {
            Ok(b) if b.len() == 1 => Ok(b[0]),
            _ => Err(format!(
                "{{{name}}}: the fill is one hex byte (xx), not '{t}'"
            )),
        },
    }
}

fn parse_dir(inner: &str, top: bool) -> Result<DirOrLayout, String> {
    let toks = dir_tokens(inner)?;
    let Some((&name, args)) = toks.split_first() else {
        return Err("an empty directive {}".into());
    };
    if let Some(kind) = HashKind::parse(name) {
        let mut ops = Vec::new();
        let mut seed = None;
        for a in args {
            if let Some(s) = a.strip_prefix("seed=") {
                if !kind.seeded() {
                    return Err(format!("{{{name}}} takes no seed"));
                }
                if seed.is_some() {
                    return Err(format!("{{{name}}}: seed= given twice"));
                }
                seed = Some(match parse_u64(s) {
                    Some(v) => Seed::Value(v),
                    None => Seed::Bytes(parse_operand(s, false).map_err(|e| {
                        format!("{{{name}}} seed: {e}; a seed is an integer or an 8-byte operand")
                    })?),
                });
            } else {
                ops.push(parse_operand(a, true).map_err(|e| format!("{{{name}}}: {e}"))?);
            }
        }
        if ops.is_empty() {
            return Err(format!(
                "{{{name}}} needs at least one operand (a range a..b)"
            ));
        }
        return Ok(DirOrLayout::Dir(Dir::Hash { kind, ops, seed }));
    }
    if name == "len" {
        let mut ops = Vec::new();
        let mut ty = IntType::U32;
        for (i, a) in args.iter().enumerate() {
            match IntType::parse(a) {
                Some(t) if i + 1 == args.len() && !ops.is_empty() => {
                    if !t.unsigned() {
                        return Err(format!(
                            "{{len}}: a length is unsigned: u8, u16, u32, u64 or uvar, not {a}"
                        ));
                    }
                    ty = t;
                }
                _ => ops.push(parse_operand(a, true).map_err(|e| format!("{{len}}: {e}"))?),
            }
        }
        if ops.is_empty() {
            return Err("{len} needs at least one operand (a range a..b)".into());
        }
        return Ok(DirOrLayout::Dir(Dir::Len { ops, ty }));
    }
    if let Some(ty) = IntType::parse(name) {
        let [v] = args else {
            return Err(format!("{{{name}}} takes one integer"));
        };
        let n = parse_int(v).ok_or_else(|| format!("{{{name}}}: '{v}' is not an integer"))?;
        return Ok(DirOrLayout::Dir(Dir::Bytes(
            ty.encode(n).map_err(|e| format!("{{{name}}}: {e}"))?,
        )));
    }
    if name == "align" || name == "pad_to" {
        if !top {
            return Err(format!("{{{name}}} cannot be nested"));
        }
        if args.is_empty() || args.len() > 2 {
            return Err(format!(
                "{{{name}}} takes a {} and an optional fill byte",
                if name == "align" { "size" } else { "position" }
            ));
        }
        let fill = fill_byte(args.get(1), name)?;
        return Ok(DirOrLayout::Layout(if name == "align" {
            let n = parse_u64(args[0])
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{{align}}: '{}' is not a size ≥ 1", args[0]))?;
            Kind::Align { n, fill }
        } else {
            Kind::PadTo {
                pos: parse_pos(args[0]).map_err(|e| format!("{{pad_to}}: {e}"))?,
                fill,
            }
        }));
    }
    Err(format!(
        "unknown directive '{name}' (xxh3_64, xxh3_128, blake3_256, blake3_128, len, u8..u64, i8..i64, uvar, svar, align, pad_to)"
    ))
}

/// A string literal starting at `s[0] == '"'`: its bytes and the index after the closing quote.
fn parse_string(s: &str) -> Result<(Vec<u8>, usize), String> {
    let mut out = Vec::new();
    let mut it = s.char_indices().skip(1);
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Ok((out, i + 1)),
            '\\' => {
                let (_, e) = it.next().ok_or("a string ends with '\\'")?;
                match e {
                    '\\' => out.push(b'\\'),
                    '"' => out.push(b'"'),
                    'n' => out.push(b'\n'),
                    'r' => out.push(b'\r'),
                    't' => out.push(b'\t'),
                    '0' => out.push(0),
                    'x' => {
                        let h: String = (0..2).filter_map(|_| it.next().map(|(_, c)| c)).collect();
                        let b = u8::from_str_radix(&h, 16)
                            .ok()
                            .filter(|_| h.len() == 2 && h.bytes().all(|c| c.is_ascii_hexdigit()))
                            .ok_or_else(|| {
                                format!("'\\x{h}' is not an escape of two hex digits")
                            })?;
                        out.push(b);
                    }
                    other => return Err(format!("unknown escape '\\{other}' in a string")),
                }
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    Err("a string without its closing '\"'".into())
}

/// The index of the `}` that closes the `{` at `s[0]`; braces inside strings are text.
fn closing_brace(s: &str) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut st = StrState::default();
    for (i, c) in s.char_indices() {
        if st.step(c) {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    Err(if st.inside {
        "a string inside a directive without its closing '\"'".into()
    } else {
        "a '{' without its '}' on the same line".into()
    })
}

fn parse_line(text: &str, line: u32, items: &mut Vec<Item>) -> Result<(), String> {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() || rest.starts_with('#') {
            return Ok(());
        }
        if rest.starts_with('"') {
            let (bytes, n) = parse_string(rest)?;
            items.push(Item {
                kind: Kind::Bytes { bytes, count: 1 },
                line,
            });
            rest = &rest[n..];
            continue;
        }
        if rest.starts_with('{') {
            let end = closing_brace(rest)?;
            let kind = match parse_dir(&rest[1..end], true)? {
                DirOrLayout::Dir(Dir::Bytes(bytes)) => Kind::Bytes { bytes, count: 1 },
                DirOrLayout::Dir(d) => Kind::Dir(d),
                DirOrLayout::Layout(k) => k,
            };
            items.push(Item { kind, line });
            rest = &rest[end + 1..];
            continue;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '#' || c == '"' || c == '{')
            .unwrap_or(rest.len());
        let tok = &rest[..end];
        rest = &rest[end..];
        let kind = if let Some(name) = tok.strip_suffix(':') {
            if !valid_label(name) {
                return Err(format!(
                    "'{tok}': a label is [A-Za-z_][A-Za-z0-9_.]*, without '..' and not ending in '.', followed by ':'"
                ));
            }
            Kind::Label(name.to_string())
        } else if let Some((h, n)) = tok.split_once('*') {
            if h.is_empty() {
                return Err(format!("'{tok}': a repetition repeats hex bytes (xx*n)"));
            }
            let count = parse_u64(n)
                .ok_or_else(|| format!("'{tok}': the repetition count '{n}' is not a number"))?;
            Kind::Bytes {
                bytes: hex_bytes(h)?,
                count,
            }
        } else {
            Kind::Bytes {
                bytes: hex_bytes(tok)?,
                count: 1,
            }
        };
        items.push(Item { kind, line });
    }
}

fn parse_pragma(text: &str) -> Result<Expect, String> {
    let text = text.split_once('#').map_or(text, |(p, _)| p);
    let toks: Vec<&str> = text.split_whitespace().collect();
    match toks.as_slice() {
        ["!expect", "blake3_256", h] => {
            let b = hex_bytes(h)
                .ok()
                .filter(|b| b.len() == 32)
                .ok_or_else(|| format!("!expect blake3_256 takes 64 hex digits, not '{h}'"))?;
            let mut a = [0u8; 32];
            a.copy_from_slice(&b);
            Ok(Expect::Blake3(a))
        }
        ["!expect", "len", n] => parse_u64(n)
            .map(Expect::Len)
            .ok_or_else(|| format!("!expect len takes a number, not '{n}'")),
        _ => Err(format!(
            "unknown pragma '{}' (!expect blake3_256 <hex> or !expect len <n>)",
            text.trim()
        )),
    }
}

/// Calls `f` for every range of an operand; see [`dir_ranges`].
fn operand_ranges<'a>(o: &'a Operand, bytes_only: bool, f: &mut dyn FnMut(&'a Pos, &'a Pos)) {
    match o {
        Operand::Range(a, b) => f(a, b),
        Operand::Nested(d) => dir_ranges(d, bytes_only, f),
        Operand::Literal(_) => {}
    }
}

/// Calls `f` for every range a directive names: its operands', its nested directives' and its seed's. With
/// `bytes_only`, only the ranges whose bytes it reads: a length reads the layout, never bytes.
fn dir_ranges<'a>(d: &'a Dir, bytes_only: bool, f: &mut dyn FnMut(&'a Pos, &'a Pos)) {
    match d {
        Dir::Hash { ops, seed, .. } => {
            for o in ops {
                operand_ranges(o, bytes_only, f);
            }
            if let Some(Seed::Bytes(o)) = seed {
                operand_ranges(o, bytes_only, f);
            }
        }
        Dir::Len { ops, .. } => {
            if !bytes_only {
                for o in ops {
                    operand_ranges(o, bytes_only, f);
                }
            }
        }
        Dir::Bytes(_) => {}
    }
}

/// Every label a directive refers to.
fn refs_of<'a>(d: &'a Dir, out: &mut Vec<&'a str>) {
    dir_ranges(d, false, &mut |a, b| {
        for p in [a, b] {
            if let Base::Label(l) = &p.base {
                out.push(l);
            }
        }
    });
}

struct Source {
    items: Vec<Item>,
    expects: Vec<(Expect, u32)>,
}

fn parse(text: &str) -> Result<Source, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut items = Vec::new();
    let mut expects = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if l.contains('\r') {
            return Err(at(line, "a CR that does not end the line"));
        }
        if l.trim_start().starts_with('!') {
            expects.push((parse_pragma(l).map_err(|e| at(line, e))?, line));
            continue;
        }
        parse_line(l, line, &mut items).map_err(|e| at(line, e))?;
    }
    // Labels are defined once, and every reference names one.
    let mut defined: HashMap<&str, u32> = HashMap::new();
    for it in &items {
        if let Kind::Label(n) = &it.kind
            && let Some(first) = defined.insert(n, it.line)
        {
            return Err(at(
                it.line,
                format!("label '{n}' is already defined on line {first}"),
            ));
        }
    }
    for it in &items {
        let mut refs = Vec::new();
        match &it.kind {
            Kind::Dir(d) => refs_of(d, &mut refs),
            Kind::PadTo { pos, .. } => {
                if let Base::Label(l) = &pos.base {
                    refs.push(l);
                }
            }
            _ => {}
        }
        if let Some(r) = refs.iter().find(|r| !defined.contains_key(**r)) {
            return Err(at(it.line, format!("no label '{r}' in this file")));
        }
    }
    Ok(Source { items, expects })
}

// ---------------------------------------------------------------------------------------------------------------
// Layout

type Labels = HashMap<String, u64>;

/// A position's offset; negative results are clamped to 0 while the layout settles and refused afterwards.
fn resolve(p: &Pos, labels: &Labels) -> Result<u64, String> {
    let base = match &p.base {
        Base::Abs(n) => *n,
        Base::Label(l) => *labels.get(l).ok_or_else(|| format!("no label '{l}'"))?,
    };
    base.checked_add_signed(p.delta).ok_or_else(|| {
        format!(
            "the position {}{:+} is before the start of the file",
            match &p.base {
                Base::Abs(n) => n.to_string(),
                Base::Label(l) => l.clone(),
            },
            p.delta
        )
    })
}

fn range_bounds(a: &Pos, b: &Pos, labels: &Labels) -> Result<(u64, u64), String> {
    let (x, y) = (resolve(a, labels)?, resolve(b, labels)?);
    if y < x {
        return Err(format!("the range {x}..{y} ends before it starts"));
    }
    Ok((x, y))
}

fn operand_len(o: &Operand, labels: &Labels, settled: bool) -> Result<u64, String> {
    match o {
        Operand::Range(a, b) => match range_bounds(a, b, labels) {
            Ok((x, y)) => Ok(y - x),
            Err(e) if settled => Err(e),
            Err(_) => Ok(0),
        },
        Operand::Nested(d) => dir_size(d, labels, settled),
        Operand::Literal(b) => Ok(b.len() as u64),
    }
}

fn ops_len(ops: &[Operand], labels: &Labels, settled: bool) -> Result<u64, String> {
    ops.iter().try_fold(0u64, |acc, o| {
        Ok(acc.saturating_add(operand_len(o, labels, settled)?))
    })
}

fn dir_size(d: &Dir, labels: &Labels, settled: bool) -> Result<u64, String> {
    Ok(match d {
        Dir::Hash { kind, .. } => kind.width(),
        Dir::Bytes(b) => b.len() as u64,
        Dir::Len { ty, ops } => match ty {
            IntType::U8 | IntType::I8 => 1,
            IntType::U16 | IntType::I16 => 2,
            IntType::U32 | IntType::I32 => 4,
            IntType::U64 | IntType::I64 => 8,
            IntType::Uvar | IntType::Svar => uvar_len(ops_len(ops, labels, settled)?),
        },
    })
}

struct Layout {
    offsets: Vec<u64>,
    sizes: Vec<u64>,
    labels: Labels,
    total: u64,
}

fn item_size(it: &Item, off: u64, labels: &Labels, settled: bool) -> Result<u64, String> {
    Ok(match &it.kind {
        Kind::Bytes { bytes, count } => (bytes.len() as u64)
            .checked_mul(*count)
            .filter(|n| *n <= MAX_OUTPUT)
            .ok_or_else(|| format!("the repetition is larger than {MAX_OUTPUT} bytes"))?,
        Kind::Label(_) => 0,
        Kind::Dir(d) => dir_size(d, labels, settled)?,
        Kind::Align { n, .. } => (n - off % n) % n,
        Kind::PadTo { pos, .. } => {
            let target = match resolve(pos, labels) {
                Ok(t) => t,
                Err(e) if settled => return Err(e),
                Err(_) => off,
            };
            if settled && target < off {
                return Err(format!(
                    "{{pad_to}}: the output is already at offset {off}, past {target}"
                ));
            }
            target.saturating_sub(off)
        }
    })
}

fn layout(items: &[Item]) -> Result<Layout, String> {
    let n = items.len();
    let mut sizes = vec![0u64; n];
    let mut offsets = vec![0u64; n];
    let mut labels: Labels = HashMap::new();
    for round in 0..=MAX_ROUNDS {
        let mut off = 0u64;
        for (i, it) in items.iter().enumerate() {
            offsets[i] = off;
            if let Kind::Label(l) = &it.kind {
                labels.insert(l.clone(), off);
            }
            off = off.saturating_add(sizes[i]);
            if off > MAX_OUTPUT {
                return Err(at(
                    it.line,
                    format!("the output grows past {MAX_OUTPUT} bytes"),
                ));
            }
        }
        let settled_before = round > 0;
        let mut changed = false;
        for (i, it) in items.iter().enumerate() {
            let s = item_size(it, offsets[i], &labels, false).map_err(|e| at(it.line, e))?;
            if s != sizes[i] {
                sizes[i] = s;
                changed = true;
            }
        }
        if !changed && settled_before {
            // Settled: validate every position and range with the final offsets.
            for (i, it) in items.iter().enumerate() {
                item_size(it, offsets[i], &labels, true).map_err(|e| at(it.line, e))?;
                if let Kind::Dir(d) = &it.kind {
                    let mut bad: Option<String> = None;
                    dir_ranges(d, false, &mut |a, b| {
                        if bad.is_none() {
                            bad = match range_bounds(a, b, &labels) {
                                Err(e) => Some(e),
                                Ok((_, y)) if y > off => Some(format!(
                                    "the range {}..{y} ends past the end of the file ({off} bytes)",
                                    resolve(a, &labels).unwrap_or(0)
                                )),
                                Ok(_) => None,
                            };
                        }
                    });
                    if let Some(e) = bad {
                        return Err(at(it.line, e));
                    }
                }
            }
            return Ok(Layout {
                offsets,
                sizes,
                labels,
                total: off,
            });
        }
    }
    Err(format!(
        "the layout does not settle after {MAX_ROUNDS} rounds: a {{len ... uvar}} or {{pad_to}} depends on its own size"
    ))
}

// ---------------------------------------------------------------------------------------------------------------
// Evaluation

/// Every output range a directive reads the bytes of (validated by the layout).
fn reads(d: &Dir, labels: &Labels) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    dir_ranges(d, true, &mut |a, b| {
        if let Ok(r) = range_bounds(a, b, labels) {
            out.push(r);
        }
    });
    out
}

/// The output bytes of a range.
fn range_slice<'b>(a: &Pos, b: &Pos, buf: &'b [u8], labels: &Labels) -> Result<&'b [u8], String> {
    let (x, y) = range_bounds(a, b, labels)?;
    let (x, y) = (
        usize::try_from(x).map_err(|e| e.to_string())?,
        usize::try_from(y).map_err(|e| e.to_string())?,
    );
    buf.get(x..y).ok_or_else(|| {
        format!(
            "the range {x}..{y} ends past the end of the file ({} bytes)",
            buf.len()
        )
    })
}

/// An operand's bytes, for a seed (at most a nested directive's few output bytes are copied).
fn operand_bytes<'a>(
    o: &'a Operand,
    buf: &'a [u8],
    labels: &Labels,
) -> Result<Cow<'a, [u8]>, String> {
    match o {
        Operand::Range(a, b) => range_slice(a, b, buf, labels).map(Cow::Borrowed),
        Operand::Nested(d) => eval(d, buf, labels).map(Cow::Owned),
        Operand::Literal(b) => Ok(Cow::Borrowed(b)),
    }
}

/// Passes each operand's bytes to `sink` in order: ranges and literals where they are, nested directives through
/// their (small) outputs, so no operand is copied.
fn feed(
    ops: &[Operand],
    buf: &[u8],
    labels: &Labels,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<(), String> {
    for o in ops {
        match o {
            Operand::Range(a, b) => sink(range_slice(a, b, buf, labels)?),
            Operand::Nested(d) => sink(&eval(d, buf, labels)?),
            Operand::Literal(b) => sink(b),
        }
    }
    Ok(())
}

fn eval(d: &Dir, buf: &[u8], labels: &Labels) -> Result<Vec<u8>, String> {
    match d {
        Dir::Bytes(b) => Ok(b.clone()),
        Dir::Len { ops, ty } => {
            let n = ops_len(ops, labels, true)?;
            ty.encode(i128::from(n))
                .map_err(|e| format!("{{len}}: {e}"))
        }
        Dir::Hash { kind, ops, seed } => {
            let seed = match seed {
                None => 0,
                Some(Seed::Value(v)) => *v,
                Some(Seed::Bytes(o)) => {
                    let b = operand_bytes(o, buf, labels)?;
                    let a: [u8; 8] = b
                        .as_ref()
                        .try_into()
                        .map_err(|_| format!("a seed operand is 8 bytes, not {}", b.len()))?;
                    u64::from_le_bytes(a)
                }
            };
            kind.digest(seed, |sink| feed(ops, buf, labels, sink))
        }
    }
}

/// The smallest hash index at or after `i` that is not yet computed (`next[k] == k`), with path compression; `next`
/// has one sentinel entry past the last hash.
fn next_pending(next: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while next[root] != root {
        root = next[root];
    }
    let mut k = i;
    while next[k] != root {
        let n = next[k];
        next[k] = root;
        k = n;
    }
    root
}

/// A hash on the evaluation stack: the next of its runs to wait for, and where in that run.
struct Frame {
    hash: usize,
    run: usize,
    cursor: usize,
}

/// Assembles a `.hex` text. Errors name their line (`line 12: …`).
///
/// Hashes are evaluated in a topological order of their reads. Output spans are disjoint and in offset order, so each
/// read range covers a contiguous run of hash outputs, found by binary search. A depth-first walk computes every hash
/// after the hashes its runs cover, skipping computed ones through [`next_pending`], so the order costs
/// O((P + R) log P) for P hashes and R read ranges. A run that reaches a hash on the walk's own stack is a cycle
/// ([F01 §7.4]).
pub fn assemble(text: &str) -> Result<Assembled, String> {
    let src = parse(text)?;
    let lay = layout(&src.items)?;
    let total = usize::try_from(lay.total).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; total];
    // The hashes in offset order: item index and output span; and the ranges each reads.
    let mut hashes: Vec<(usize, (u64, u64))> = Vec::new();
    let mut hash_reads: Vec<Vec<(u64, u64)>> = Vec::new();
    for (i, it) in src.items.iter().enumerate() {
        let start = usize::try_from(lay.offsets[i]).map_err(|e| e.to_string())?;
        let size = usize::try_from(lay.sizes[i]).map_err(|e| e.to_string())?;
        let span = &mut buf[start..start + size];
        match &it.kind {
            Kind::Bytes { bytes, .. } => {
                if !bytes.is_empty() {
                    for chunk in span.chunks_mut(bytes.len()) {
                        chunk.copy_from_slice(bytes);
                    }
                }
            }
            Kind::Label(_) => {}
            Kind::Align { fill, .. } | Kind::PadTo { fill, .. } => span.fill(*fill),
            Kind::Dir(d @ Dir::Hash { .. }) => {
                hashes.push((i, (lay.offsets[i], lay.offsets[i] + lay.sizes[i])));
                hash_reads.push(reads(d, &lay.labels));
            }
            Kind::Dir(d) => {
                let v = eval(d, &[], &lay.labels).map_err(|e| at(it.line, e))?;
                if v.len() != size {
                    return Err(at(
                        it.line,
                        "internal: a directive's size changed after the layout",
                    ));
                }
                span.copy_from_slice(&v);
            }
        }
    }
    // Each hash's runs: the half-open index ranges of the hash outputs its reads overlap.
    let p = hashes.len();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut first_run: Vec<usize> = Vec::with_capacity(p + 1);
    for (h, r) in hash_reads.iter().enumerate() {
        first_run.push(runs.len());
        for &(x, y) in r {
            let lo = hashes.partition_point(|(_, o)| o.1 <= x);
            let hi = hashes.partition_point(|(_, o)| o.0 < y);
            if lo >= hi {
                continue;
            }
            if (lo..hi).contains(&h) {
                return Err(at(
                    src.items[hashes[h].0].line,
                    "a hash reads its own output bytes (F01 §7.4: a checksum is never inside its own range)",
                ));
            }
            runs.push((lo, hi));
        }
    }
    first_run.push(runs.len());
    drop(hash_reads);
    let mut next: Vec<usize> = (0..=p).collect();
    let mut on_stack = vec![false; p];
    let mut stack: Vec<Frame> = Vec::new();
    for root in 0..p {
        if next_pending(&mut next, root) != root {
            continue;
        }
        on_stack[root] = true;
        stack.push(Frame {
            hash: root,
            run: first_run[root],
            cursor: 0,
        });
        while let Some(f) = stack.last_mut() {
            let mut dep = None;
            while f.run < first_run[f.hash + 1] {
                let (lo, hi) = runs[f.run];
                let c = next_pending(&mut next, f.cursor.max(lo));
                if c < hi {
                    f.cursor = c;
                    dep = Some(c);
                    break;
                }
                f.run += 1;
                f.cursor = 0;
            }
            let h = f.hash;
            match dep {
                Some(d) if on_stack[d] => {
                    let from = stack.iter().position(|f| f.hash == d).unwrap_or(0);
                    let lines: Vec<String> = stack[from..]
                        .iter()
                        .map(|f| src.items[hashes[f.hash].0].line.to_string())
                        .collect();
                    return Err(format!(
                        "the hashes on lines {} read each other's output: no order computes them",
                        lines.join(", ")
                    ));
                }
                Some(d) => {
                    on_stack[d] = true;
                    stack.push(Frame {
                        hash: d,
                        run: first_run[d],
                        cursor: 0,
                    });
                }
                None => {
                    let (i, out) = hashes[h];
                    let it = &src.items[i];
                    if let Kind::Dir(d) = &it.kind {
                        let v = eval(d, &buf, &lay.labels).map_err(|e| at(it.line, e))?;
                        let (s, e) = (
                            usize::try_from(out.0).map_err(|e| e.to_string())?,
                            usize::try_from(out.1).map_err(|e| e.to_string())?,
                        );
                        buf[s..e].copy_from_slice(&v);
                    }
                    on_stack[h] = false;
                    next[h] = h + 1;
                    stack.pop();
                }
            }
        }
    }
    Ok(Assembled {
        bytes: buf,
        expects: src.expects,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Files and `--check`

/// `<path>.hex` → `<path>.bin`.
pub fn bin_path(hex: &Path) -> PathBuf {
    hex.with_extension("bin")
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// Reads and assembles one file.
pub fn assemble_file(path: &Path) -> Result<Assembled, String> {
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(raw).map_err(|_| format!("{}: not UTF-8", path.display()))?;
    assemble(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            walk(&e.path(), out)?;
        } else if ft.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct CheckReport {
    pub diags: Vec<Diag>,
    /// `.hex` files assembled.
    pub files: usize,
    pub notice: Option<String>,
}

/// `xtask hex --check`: every `.hex` under `paths` (default `fixtures/hex`) against its `.bin`, which it must have,
/// and its `!expect` lines, and every `.bin` found in a walked directory against the presence of its `.hex`.
pub fn check(repo: &Path, paths: &[PathBuf]) -> Result<CheckReport, String> {
    let mut report = CheckReport::default();
    let default = repo.join("fixtures/hex");
    let roots: Vec<PathBuf> = if paths.is_empty() {
        if !default.is_dir() {
            report.notice =
                Some("fixtures/hex does not exist yet (WP-20): nothing to check".into());
            return Ok(report);
        }
        vec![default]
    } else {
        paths.to_vec()
    };
    let mut hexes = Vec::new();
    let mut bins = Vec::new();
    for r in &roots {
        if r.is_dir() {
            let mut files = Vec::new();
            walk(r, &mut files)?;
            for f in files {
                if has_ext(&f, "hex") {
                    hexes.push(f);
                } else if has_ext(&f, "bin") {
                    bins.push(f);
                }
            }
        } else if r.is_file() && has_ext(r, "hex") {
            hexes.push(r.clone());
        } else {
            return Err(format!("{}: not a .hex file or a directory", r.display()));
        }
    }
    hexes.sort();
    bins.sort();
    let rel = |p: &Path| -> String {
        let s = p.strip_prefix(repo).unwrap_or(p);
        s.to_string_lossy().replace('\\', "/")
    };
    for h in &hexes {
        report.files += 1;
        let hp = rel(h);
        let a = match assemble_file(h) {
            Ok(a) => a,
            Err(e) => {
                let msg = e
                    .strip_prefix(&format!("{}: ", h.display()))
                    .unwrap_or(&e)
                    .to_string();
                report.diags.push(Diag::path("hex", &hp, msg));
                continue;
            }
        };
        for m in a.mismatches() {
            report.diags.push(Diag::path("hex", &hp, m));
        }
        let b = bin_path(h);
        if b.is_file() {
            let committed = std::fs::read(&b).map_err(|e| format!("{}: {e}", b.display()))?;
            if committed != a.bytes {
                let first = committed
                    .iter()
                    .zip(&a.bytes)
                    .position(|(x, y)| x != y)
                    .unwrap_or(committed.len().min(a.bytes.len()));
                report.diags.push(Diag::path(
                    "hex",
                    &hp,
                    format!(
                        "differs from {}: first difference at offset {first} (0x{first:x}); assembled {} bytes, committed {}; re-run cargo xtask hex {hp}",
                        rel(&b),
                        a.bytes.len(),
                        committed.len()
                    ),
                ));
            }
        } else {
            report.diags.push(Diag::path(
                "hex",
                &hp,
                format!(
                    "no {} beside it: the committed .bin is what the format oracle and the codec read (docs/m0/authors.md §6 item 3); write it with cargo xtask hex {hp} and commit it",
                    rel(&b)
                ),
            ));
        }
    }
    for b in &bins {
        if !b.with_extension("hex").is_file() {
            report.diags.push(Diag::path(
                "hex",
                &rel(b),
                "a .bin with no .hex beside it: every file here is assembled from a .hex",
            ));
        }
    }
    Ok(report)
}

/// `cargo xtask hex` ([`USAGE`]): `args` follow the subcommand, and file names are relative to `cwd`; `repo` gives
/// the repository root for `--check`. Every file is processed and reported, whatever happened to the ones before
/// it. Returns whether every file passed; a usage error is an `Err`.
pub fn cli(
    args: &[String],
    cwd: &Path,
    repo: &dyn Fn() -> Result<PathBuf, String>,
    out: &mut dyn Write,
) -> Result<bool, String> {
    let io = |e: std::io::Error| format!("hex: output: {e}");
    if args.iter().any(|a| a == "--help" || a == "-h") {
        writeln!(out, "{USAGE}").map_err(io)?;
        return Ok(true);
    }
    let (mut check_mode, mut digest) = (false, false);
    let mut dest: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let o = match a.as_str() {
            "--check" => {
                check_mode = true;
                continue;
            }
            "--digest" => {
                digest = true;
                continue;
            }
            "-o" => it.next().ok_or("hex: -o needs a value")?.as_str(),
            _ => match a.strip_prefix("-o=") {
                Some(v) => v,
                None if a.starts_with('-') => {
                    return Err(format!("hex: unknown option '{a}'\n{USAGE}"));
                }
                None => {
                    files.push(cwd.join(a));
                    continue;
                }
            },
        };
        if dest.replace(cwd.join(o)).is_some() {
            return Err("hex: -o given twice".into());
        }
    }
    if check_mode {
        if digest || dest.is_some() {
            return Err("hex: --check takes neither --digest nor -o".into());
        }
        let r = check(&repo()?, &files)?;
        for d in &r.diags {
            writeln!(out, "{d}").map_err(io)?;
        }
        match &r.notice {
            Some(n) => writeln!(out, "hex: {n}"),
            None => writeln!(
                out,
                "hex: {} .hex files checked, {} findings",
                r.files,
                r.diags.len()
            ),
        }
        .map_err(io)?;
        return Ok(r.diags.is_empty());
    }
    if files.is_empty() {
        return Err(format!("hex: no input file\n{USAGE}"));
    }
    if dest.is_some() && (files.len() != 1 || digest) {
        return Err("hex: -o takes exactly one input and no --digest".into());
    }
    let mut ok = true;
    for f in &files {
        let asm = match assemble_file(f) {
            Ok(a) => a,
            Err(e) => {
                writeln!(out, "{e}").map_err(io)?;
                if !digest {
                    writeln!(out, "{}: not written", f.display()).map_err(io)?;
                }
                ok = false;
                continue;
            }
        };
        let blake = hex_str(blake3::hash(&asm.bytes).as_bytes());
        let bad = asm.mismatches();
        for m in &bad {
            writeln!(out, "{}: {m}", f.display()).map_err(io)?;
        }
        ok &= bad.is_empty();
        if digest {
            writeln!(
                out,
                "{}: {} bytes, blake3_256 {blake}{}",
                f.display(),
                asm.bytes.len(),
                if bad.is_empty() {
                    ""
                } else {
                    "; its !expect lines disagree (above)"
                }
            )
            .map_err(io)?;
            continue;
        }
        if !bad.is_empty() {
            writeln!(
                out,
                "{}: not written; update its !expect lines (length {}, blake3_256 {blake})",
                f.display(),
                asm.bytes.len()
            )
            .map_err(io)?;
            continue;
        }
        let dst = dest.clone().unwrap_or_else(|| bin_path(f));
        match std::fs::write(&dst, &asm.bytes) {
            Ok(()) => writeln!(
                out,
                "{} -> {}: {} bytes, blake3_256 {blake}",
                f.display(),
                dst.display(),
                asm.bytes.len()
            ),
            Err(e) => {
                ok = false;
                writeln!(out, "{}: not written: {e}", dst.display())
            }
        }
        .map_err(io)?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use proptest::prelude::*;
    use xxhash_rust::xxh3;

    fn asm(t: &str) -> Vec<u8> {
        match assemble(t) {
            Ok(a) => a.bytes,
            Err(e) => panic!("{e}\n--- in ---\n{t}"),
        }
    }

    fn err(t: &str) -> String {
        match assemble(t) {
            Ok(a) => panic!("assembled {:02x?} from\n{t}", a.bytes),
            Err(e) => e,
        }
    }

    #[test]
    fn bytes_strings_repetition_and_comments() {
        assert_eq!(asm("4d 4F4952 # MOIR\n"), b"MOIR");
        assert_eq!(asm("\"MOIR\"01"), b"MOIR\x01");
        assert_eq!(asm("00*3 ab*2\n\n"), [0, 0, 0, 0xab, 0xab]);
        assert_eq!(asm("0102*0x2"), [1, 2, 1, 2]);
        assert_eq!(
            asm("\"a#b\\\"\\\\\\n\\r\\t\\0\\x7f\" # tail"),
            b"a#b\"\\\n\r\t\0\x7f"
        );
        assert_eq!(asm("\"é\""), "é".as_bytes());
        assert_eq!(asm("\u{feff}01\r\n02\r\n"), [1, 2]);
        assert!(asm("# only a comment\n").is_empty());
    }

    #[test]
    fn integers_follow_f01() {
        // [F01 §4.1] examples.
        assert_eq!(asm("{u32 0x0A0B0C0D}"), [0x0d, 0x0c, 0x0b, 0x0a]);
        assert_eq!(asm("{u64 0x0102030405060708}"), [8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(
            asm("{i32 -1} {i8 -128} {u16 65535}"),
            [0xff, 0xff, 0xff, 0xff, 0x80, 0xff, 0xff]
        );
        // [F01 §5.2] uvar examples.
        for (v, b) in [
            ("0", vec![0x00]),
            ("1", vec![0x01]),
            ("127", vec![0x7f]),
            ("128", vec![0x80, 0x01]),
            ("300", vec![0xac, 0x02]),
            ("16383", vec![0xff, 0x7f]),
            ("16384", vec![0x80, 0x80, 0x01]),
            ("65535", vec![0xff, 0xff, 0x03]),
            ("4294967295", vec![0xff, 0xff, 0xff, 0xff, 0x0f]),
            (
                "18446744073709551615",
                vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
        ] {
            assert_eq!(asm(&format!("{{uvar {v}}}")), b, "uvar {v}");
        }
        // [F01 §5.3] svar examples.
        for (v, b) in [
            ("0", vec![0x00]),
            ("-1", vec![0x01]),
            ("1", vec![0x02]),
            ("-64", vec![0x7f]),
            ("64", vec![0x80, 0x01]),
            ("-65", vec![0x81, 0x01]),
        ] {
            assert_eq!(asm(&format!("{{svar {v}}}")), b, "svar {v}");
        }
        assert!(err("{u8 256}").contains("does not fit u8"));
        assert!(err("{u64 -1}").contains("does not fit u64"));
        assert!(err("{i8 128}").contains("does not fit i8"));
        assert!(err("{u32}").contains("takes one integer"));
    }

    #[test]
    fn lengths_and_forward_labels() {
        // [F01 §6.3]: lp("moirai-file-v1").
        assert_eq!(
            asm("{len s..e} s: \"moirai-file-v1\" e:"),
            [
                0x0e, 0, 0, 0, 0x6d, 0x6f, 0x69, 0x72, 0x61, 0x69, 0x2d, 0x66, 0x69, 0x6c, 0x65,
                0x2d, 0x76, 0x31
            ]
        );
        // [F01 §6.2]: `docs/a.md` as vstr.
        assert_eq!(
            asm("{len a..b uvar} a: \"docs/a.md\" b:"),
            [0x09, 0x64, 0x6f, 0x63, 0x73, 0x2f, 0x61, 0x2e, 0x6d, 0x64]
        );
        assert_eq!(asm("{len a..b a..b u8} a: 00*3 b:"), [6, 0, 0, 0]);
        assert_eq!(asm("{len {u64 0} u16}"), [8, 0]);
        assert_eq!(asm("x: 01 {len 0..x+1 u8}"), [1, 1]);
        assert!(err("{len a..b u8} a: 00*256 b:").contains("does not fit u8"));
        assert!(err("{len a..b i32} a: b:").contains("unsigned"));
        // A uvar length that covers itself settles: 127 bytes and a 1-byte length are 128, which needs 2 bytes, so
        // 129 = 81 01.
        let v = asm("s: {len s..e uvar} 00*127 e:");
        assert_eq!(&v[..2], &[0x81, 0x01]);
        assert_eq!(v.len(), 129);
        assert_eq!(asm("s: {len s..e uvar} 00*126 e:")[0], 0x7f);
    }

    #[test]
    fn hashes_match_their_definitions() {
        let data = b"hello, fixture";
        let t = "a: \"hello, fixture\" b: {xxh3_64 a..b} {xxh3_128 a..b} {blake3_256 a..b} {blake3_128 a..b}";
        let v = asm(t);
        let mut want = data.to_vec();
        want.extend_from_slice(&xxh3::xxh3_64(data).to_le_bytes());
        let h128 = xxh3::xxh3_128(data);
        want.extend_from_slice(&(h128 as u64).to_le_bytes());
        want.extend_from_slice(&((h128 >> 64) as u64).to_le_bytes());
        want.extend_from_slice(blake3::hash(data).as_bytes());
        want.extend_from_slice(&blake3::hash(data).as_bytes()[..16]);
        assert_eq!(v, want);
        // Seeds: a value, an 8-byte range and a nested directive.
        let v = asm("a: \"xy\" b: {xxh3_64 a..b seed=7} {xxh3_128 a..b seed=0x10}");
        assert_eq!(&v[2..10], &xxh3::xxh3_64_with_seed(b"xy", 7).to_le_bytes());
        assert_eq!(
            &v[10..26],
            &xxh3::xxh3_128_with_seed(b"xy", 16).to_le_bytes()
        );
        let seed = xxh3::xxh3_64(&5u64.to_le_bytes());
        let v = asm(
            "e: {u64 5} x: \"body\" y: {xxh3_64 x..y seed={xxh3_64 e..x}} {xxh3_64 x..y seed={xxh3_64 {u64 5}}}",
        );
        assert_eq!(
            &v[12..20],
            &xxh3::xxh3_64_with_seed(b"body", seed).to_le_bytes()
        );
        assert_eq!(&v[12..20], &v[20..28]);
        let v = asm("p: 11*8 x: \"q\" y: {xxh3_64 x..y seed=p..x}");
        assert_eq!(
            &v[9..],
            &xxh3::xxh3_64_with_seed(b"q", 0x1111_1111_1111_1111).to_le_bytes()
        );
        assert!(err("a: 01 b: {xxh3_64 a..b seed=a..b}").contains("8 bytes"));
        assert!(err("a: b: {blake3_256 a..b seed=1}").contains("takes no seed"));
        assert!(err("{xxh3_64}").contains("at least one operand"));
    }

    #[test]
    fn a_record_checksum_over_two_ranges_and_hashes_of_hashes() {
        // A header whose checksum at 24..32 covers [0, 24) and [32, end) ([F05 §3.4]'s shape), and a trailer over
        // everything before it that includes the checksum: evaluation order follows the reads.
        let t = "r: 01*24 c: {xxh3_64 r..c d..e} d: \"payload\" e: {xxh3_64 r..e}";
        let v = asm(t);
        let mut cat = vec![1u8; 24];
        cat.extend_from_slice(b"payload");
        assert_eq!(&v[24..32], &xxh3::xxh3_64(&cat).to_le_bytes());
        assert_eq!(&v[39..47], &xxh3::xxh3_64(&v[..39]).to_le_bytes());
        // A digest over another digest that comes later in the file.
        let v = asm("h: {blake3_256 x..y} x: {xxh3_64 z..w} y: z: \"abc\" w:");
        assert_eq!(&v[..32], blake3::hash(&v[32..40]).as_bytes());
    }

    #[test]
    fn a_slot_padded_to_its_checksum() {
        // A 4096-byte slot with its XXH3-128 at 4080 over [0, 4080) ([F04 §3]'s shape), then a second slot.
        let v = asm(
            "a: \"MOIR\" {u64 1} {pad_to a+4080} {xxh3_128 a..a+4080}\nb: \"MOIR\" {u64 2} {pad_to b+4080 00} {xxh3_128 b..b+4080}",
        );
        assert_eq!(v.len(), 8192);
        assert_eq!(&v[4080..4096], &xxh3::xxh3_128(&v[..4080]).to_le_bytes());
        assert_eq!(&v[8176..], &xxh3::xxh3_128(&v[4096..8176]).to_le_bytes());
        assert_eq!(
            asm("01 {align 4} 02 {align 4 ff} {align 1}"),
            [1, 0, 0, 0, 2, 0xff, 0xff, 0xff]
        );
        assert!(err("00*9 {pad_to 8}").contains("past 8"));
        assert!(err("{align 0}").contains("size ≥ 1"));
        assert!(err("{pad_to 4 1}").contains("one hex byte"));
    }

    #[test]
    fn refusals_name_their_line() {
        assert!(err("01\n0").starts_with("line 2: "));
        assert!(err("zz").contains("not hex bytes"));
        assert!(err("+f").contains("not hex bytes"));
        assert!(err("*4").contains("repeats hex bytes"));
        assert!(err("00*0x+1").contains("not a number"));
        assert!(err(r#""\x+1""#).contains("two hex digits"));
        assert!(err("{blake3_256 a..nope}\na:").contains("no label 'nope'"));
        assert!(err("a: 01\na: 02").contains("already defined on line 1"));
        assert!(err("9a: 01").contains("a label is"));
        assert!(err("a: {xxh3_64 a..b} b:").contains("its own output"));
        assert!(err("x: {xxh3_64 y..z} y: {xxh3_64 x..y} z:").contains("read each other's output"));
        assert!(err("{blake3_256 b..a} a: 01 b:").contains("ends before it starts"));
        assert!(err("01 {len 0..9 u8}").contains("past the end"));
        assert!(err("a: 01 {len a-1..a u8}").contains("before the start"));
        assert!(err("{xxh3_64 a..b").contains("without its '}'"));
        assert!(err("\"abc").contains("closing"));
        assert!(err("\"\\q\"").contains("unknown escape"));
        assert!(err("{nope 1}").contains("unknown directive"));
        assert!(err("{blake3_256 {align 4}}").contains("cannot be nested"));
        assert!(err("{blake3_256 x}").contains("a range a..b, a nested"));
        assert!(err("00*0x10000000").contains("larger than"));
        assert!(err("!bogus").contains("unknown pragma"));
        assert!(err("01\r02").contains("CR"));
        assert!(err("{pad_to e+1} e:").contains("does not settle"));
    }

    #[test]
    fn expect_pragmas() {
        let good = hex_str(blake3::hash(b"ab").as_bytes());
        let a = assemble(&format!(
            "!expect blake3_256 {good} # the digest\n!expect len 2\n\"ab\""
        ))
        .unwrap();
        assert!(a.mismatches().is_empty());
        let a = assemble(&format!("!expect blake3_256 {good}\n!expect len 3\n\"ac\"")).unwrap();
        let m = a.mismatches();
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(m[0].starts_with("line 1: ") && m[1].starts_with("line 2: "));
        assert!(err("!expect blake3_256 00").contains("64 hex digits"));
    }

    #[test]
    fn check_compares_with_bins_and_digests() {
        let t = TestDir::new("hex-check");
        let d = t.path();
        let hex = d.join("fixtures/hex");
        t.write("fixtures/hex/ok.hex", "01 02\n");
        t.write("fixtures/hex/ok.bin", [1u8, 2]);
        t.write("fixtures/hex/stale.hex", "01 03\n");
        t.write("fixtures/hex/stale.bin", [1u8, 2]);
        let digest = hex_str(blake3::hash(&[9u8]).as_bytes());
        // An `!expect` line is an extra pin, never a substitute for the committed .bin.
        t.write(
            "fixtures/hex/sub/pinned.hex",
            format!("!expect blake3_256 {digest}\n09\n"),
        );
        t.write("fixtures/hex/sub/pinned.bin", [9u8]);
        t.write(
            "fixtures/hex/sub/digest_only.hex",
            format!("!expect blake3_256 {digest}\n09\n"),
        );
        t.write("fixtures/hex/sub/nothing.hex", "09\n");
        t.write("fixtures/hex/sub/orphan.bin", [0u8]);
        t.write("fixtures/hex/sub/broken.hex", "0\n");
        t.write(
            "fixtures/hex/sub/wrong_pin.hex",
            format!("!expect blake3_256 {digest}\n08\n"),
        );
        t.write("fixtures/hex/sub/wrong_pin.bin", [8u8]);
        let r = check(d, &[]).unwrap();
        assert_eq!(r.files, 7);
        let msgs: Vec<String> = r.diags.iter().map(ToString::to_string).collect();
        assert_eq!(r.diags.len(), 6, "{msgs:#?}");
        let has = |file: &str, what: &str| {
            msgs.iter()
                .any(|m| m.contains(&format!("fixtures/hex/{file}")) && m.contains(what))
        };
        assert!(has("stale.hex", "offset 1"));
        assert!(has(
            "sub/nothing.hex",
            "no fixtures/hex/sub/nothing.bin beside it"
        ));
        assert!(has(
            "sub/digest_only.hex",
            "no fixtures/hex/sub/digest_only.bin"
        ));
        assert!(has("sub/orphan.bin", "no .hex"));
        assert!(has("sub/broken.hex", "line 1"));
        assert!(has("sub/wrong_pin.hex", "!expect blake3_256"));
        // One file, and a repository without fixtures/hex.
        let r = check(d, &[hex.join("ok.hex")]).unwrap();
        assert!(r.diags.is_empty() && r.files == 1);
        let r = check(d, &[hex.join("sub/pinned.hex")]).unwrap();
        assert!(r.diags.is_empty());
        let empty = t.write("empty/.keep", "");
        let r = check(empty.parent().unwrap(), &[]).unwrap();
        assert!(r.diags.is_empty() && r.notice.is_some());
    }

    #[test]
    fn literal_operands() {
        // [F01 §6.3] lp(s) = u32 length ‖ s, as the domain prefix of a derivation over stored bytes ([F01 §7.1]).
        let v = asm(
            "{blake3_128 {len \"moirai-file-v1\"} \"moirai-file-v1\" f..g} f: \"docs/a.md\" g:",
        );
        let mut want = 14u32.to_le_bytes().to_vec();
        want.extend_from_slice(b"moirai-file-v1docs/a.md");
        assert_eq!(&v[..16], &blake3::hash(&want).as_bytes()[..16]);
        // The same framing written with escapes and as hex bytes.
        assert_eq!(
            asm("{blake3_128 \"\\x0e\\0\\0\\0moirai-file-v1\" f..g} f: \"docs/a.md\" g:"),
            v
        );
        assert_eq!(
            asm("{blake3_128 0e000000 \"moirai-file-v1\" f..g} f: \"docs/a.md\" g:"),
            v
        );
        // Strings keep their spaces, braces and '#'; a literal is not part of the output.
        let v = asm("{xxh3_64 \"a b}{#\" 00}");
        assert_eq!(v, xxh3::xxh3_64(b"a b}{#\0").to_le_bytes());
        assert_eq!(asm("{len \"abc\" 0102 u8}"), [5]);
        // A seed written as a string of 8 bytes; digits after seed= are an integer.
        let v = asm("{xxh3_64 \"q\" seed=\"\\x01\\0\\0\\0\\0\\0\\0\\0\"} {xxh3_64 \"q\" seed=1}");
        assert_eq!(&v[..8], &v[8..]);
        assert!(err("{xxh3_64 \"q\" seed=ab}").contains("a seed is an integer"));
        assert!(err("{blake3_256 \"a\"b}").contains("after the string"));
        assert!(err("{blake3_256 \"a}").contains("closing"));
        assert!(err("{blake3_256 0g}").contains("a range a..b, a nested"));
        assert!(err("{blake3_256 abc}").contains("odd number"));
    }

    #[test]
    fn labels_refuse_double_and_trailing_dots() {
        assert_eq!(asm("a.b: 01 {len a.b..c.d u8} c.d:"), [1, 2]);
        assert!(err("a..b: 01").contains("without '..'"));
        assert!(err("a.: 01").contains("not ending in '.'"));
    }

    /// The chain shape the review measured: per group a header, a record checksum over the header and the payload
    /// (two ranges around it) and a seeded trailer over everything before it.
    fn chain(groups: usize) -> String {
        let mut t = String::new();
        for g in 0..groups {
            t.push_str(&format!(
                "h{g}: {{u32 {g}}} c{g}: {{xxh3_64 h{g}..c{g} p{g}..q{g}}} p{g}: \"payload\" q{g}: {{xxh3_64 0..q{g} seed={g}}}\n"
            ));
        }
        t
    }

    #[test]
    fn evaluation_follows_the_reads_in_any_order() {
        let groups = 300;
        let v = asm(&chain(groups));
        // Recompute in file order, which is a valid order for this shape.
        let mut want = Vec::new();
        for g in 0..groups {
            let h = want.len();
            want.extend_from_slice(&(g as u32).to_le_bytes());
            let mut rec = want[h..].to_vec();
            rec.extend_from_slice(b"payload");
            want.extend_from_slice(&xxh3::xxh3_64(&rec).to_le_bytes());
            want.extend_from_slice(b"payload");
            let t = xxh3::xxh3_64_with_seed(&want, g as u64);
            want.extend_from_slice(&t.to_le_bytes());
        }
        assert_eq!(v, want);
        // A backward chain: each hash reads the next one's output, so the last is computed first (a deep walk).
        let n = 2000;
        let mut t = String::new();
        for k in 0..n {
            t.push_str(&format!("x{k}: {{xxh3_64 x{}..x{}}}\n", k + 1, k + 2));
        }
        t.push_str(&format!("x{n}: \"end\" x{}:\n", n + 1));
        let v = asm(&t);
        let mut next = xxh3::xxh3_64(b"end").to_le_bytes();
        for k in (0..n).rev() {
            assert_eq!(&v[8 * k..8 * k + 8], &next, "hash {k}");
            if k > 0 {
                next = xxh3::xxh3_64(&v[8 * k..8 * k + 8]).to_le_bytes();
            }
        }
        // A cycle names the hashes on it, not the ones merely waiting for it.
        let e = err("w: {xxh3_64 x..y}\nx: {xxh3_64 y..z}\ny: {xxh3_64 x..y}\nz:");
        assert!(e.contains("lines 2, 3 read each other's output"), "{e}");
    }

    fn run_cli(t: &TestDir, args: &[&str]) -> (Result<bool, String>, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let mut out = Vec::new();
        let root = t.path().to_path_buf();
        let r = cli(&args, t.path(), &move || Ok(root.clone()), &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_command_line() {
        let t = TestDir::new("hex-cli");
        t.write("a.hex", "0\n");
        t.write("b.hex", "01 02\n");
        t.write("c.hex", "!expect len 3\n01\n");
        // A file that does not assemble stops nothing: b is still written, and the run fails.
        let (r, out) = run_cli(&t, &["a.hex", "b.hex"]);
        assert_eq!(r, Ok(false), "{out}");
        assert!(
            out.contains("a.hex: line 1") && out.contains("a.hex: not written"),
            "{out}"
        );
        assert_eq!(std::fs::read(t.path().join("b.bin")).unwrap(), [1, 2]);
        assert!(!t.path().join("a.bin").exists());
        // An !expect mismatch: not written, and the other file is.
        std::fs::remove_file(t.path().join("b.bin")).unwrap();
        let (r, out) = run_cli(&t, &["c.hex", "b.hex"]);
        assert_eq!(r, Ok(false));
        assert!(
            out.contains("c.hex: not written; update its !expect lines"),
            "{out}"
        );
        assert!(!t.path().join("c.bin").exists() && t.path().join("b.bin").exists());
        // -o names the one output.
        let (r, _) = run_cli(&t, &["-o", "out.bin", "b.hex"]);
        assert_eq!(r, Ok(true));
        assert_eq!(std::fs::read(t.path().join("out.bin")).unwrap(), [1, 2]);
        let (r, _) = run_cli(&t, &["b.hex", "-o=out2.bin"]);
        assert_eq!(r, Ok(true));
        assert!(t.path().join("out2.bin").exists());
        // --digest writes nothing and words a mismatch as a digest, not as "not written".
        std::fs::remove_file(t.path().join("b.bin")).unwrap();
        let (r, out) = run_cli(&t, &["--digest", "b.hex", "c.hex"]);
        assert_eq!(r, Ok(false));
        let want = hex_str(blake3::hash(&[1u8, 2]).as_bytes());
        assert!(
            out.contains(&format!("b.hex: 2 bytes, blake3_256 {want}")),
            "{out}"
        );
        assert!(out.contains("c.hex: 1 bytes") && out.contains("its !expect lines disagree"));
        assert!(!out.contains("not written"), "{out}");
        assert!(!t.path().join("b.bin").exists() && !t.path().join("c.bin").exists());
        // --check over a directory, and the refused combinations.
        t.write("fx/ok.hex", "05\n");
        t.write("fx/ok.bin", [5u8]);
        let (r, out) = run_cli(&t, &["--check", "fx"]);
        assert_eq!(r, Ok(true), "{out}");
        assert!(out.contains("1 .hex files checked, 0 findings"));
        for bad in [
            &["--check", "-o", "x.bin"][..],
            &["--check", "--digest"],
            &["-o", "x.bin", "a.hex", "b.hex"],
            &["--digest", "-o", "x.bin", "b.hex"],
            &["-o", "x.bin", "-o", "y.bin", "b.hex"],
            &["-o"],
            &["--bogus", "b.hex"],
            &[],
        ] {
            assert!(run_cli(&t, bad).0.is_err(), "{bad:?}");
        }
        let (r, out) = run_cli(&t, &["--help"]);
        assert_eq!(r, Ok(true));
        assert!(out.contains(".hex format"));
    }

    fn decode_uvar(b: &[u8]) -> (u64, usize) {
        let mut v = 0u64;
        for (i, x) in b.iter().enumerate() {
            v |= u64::from(x & 0x7f) << (7 * i);
            if x & 0x80 == 0 {
                return (v, i + 1);
            }
        }
        panic!("unterminated");
    }

    /// Renders bytes as `.hex` text with arbitrary spacing and case.
    fn render(bytes: &[u8], gaps: &[u8], upper: bool) -> String {
        let mut s = String::new();
        for (i, b) in bytes.iter().enumerate() {
            if upper {
                s.push_str(&format!("{b:02X}"));
            } else {
                s.push_str(&format!("{b:02x}"));
            }
            match gaps.get(i).copied().unwrap_or(0) % 4 {
                0 => {}
                1 => s.push(' '),
                2 => s.push_str(" # c\n"),
                _ => s.push('\n'),
            }
        }
        s
    }

    proptest! {
        #[test]
        fn hex_text_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..300),
                                gaps in proptest::collection::vec(any::<u8>(), 0..300),
                                upper in any::<bool>()) {
            prop_assert_eq!(asm(&render(&bytes, &gaps, upper)), bytes);
        }

        #[test]
        fn varints_are_shortest_and_decode(v in any::<u64>(), i in any::<i64>()) {
            let b = asm(&format!("{{uvar {v}}}"));
            prop_assert_eq!(decode_uvar(&b), (v, b.len()));
            prop_assert!(b.len() == 1 || *b.last().unwrap() != 0);
            let z = asm(&format!("{{svar {i}}}"));
            let (u, _) = decode_uvar(&z);
            prop_assert_eq!(((u >> 1) as i64) ^ -((u & 1) as i64), i);
        }

        #[test]
        fn directives_over_random_ranges(parts in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..40), 1..6),
                                         pick in any::<(usize, usize)>(), seed in any::<u64>()) {
            // Labels l0..ln around the parts; hashes and lengths over a random label range, placed first.
            let n = parts.len();
            let (mut x, mut y) = (pick.0 % (n + 1), pick.1 % (n + 1));
            if y < x { std::mem::swap(&mut x, &mut y); }
            let mut t = format!("{{xxh3_64 l{x}..l{y} seed={seed}}} {{blake3_128 l{x}..l{y}}} {{len l{x}..l{y} uvar}} {{xxh3_128 l{x}..l{y} l0..l{n}}}\n");
            for (i, p) in parts.iter().enumerate() {
                t.push_str(&format!("l{i}: {}\n", render(p, &[1; 40], false)));
            }
            t.push_str(&format!("l{n}:\n"));
            let v = asm(&t);
            let body: Vec<u8> = parts.concat();
            let off: Vec<usize> = std::iter::once(0).chain(parts.iter().scan(0, |a, p| { *a += p.len(); Some(*a) })).collect();
            let sel = &body[off[x]..off[y]];
            let head = 8 + 16 + uvar(sel.len() as u64).len() + 16;
            prop_assert_eq!(&v[head..], &body[..]);
            prop_assert_eq!(&v[..8], &xxh3::xxh3_64_with_seed(sel, seed).to_le_bytes());
            let b3 = blake3::hash(sel);
            prop_assert_eq!(&v[8..24], &b3.as_bytes()[..16]);
            let ul = uvar(sel.len() as u64);
            prop_assert_eq!(&v[24..24 + ul.len()], &ul[..]);
            let mut cat = sel.to_vec();
            cat.extend_from_slice(&body);
            prop_assert_eq!(&v[24 + ul.len()..head], &xxh3::xxh3_128(&cat).to_le_bytes());
        }
    }
}
