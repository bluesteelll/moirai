//! `cargo xtask ucd [--check]`: the `fold_v1` table generator (WP-61, role R-FL1A; docs/m0/PLAN.md §2.4, §6.2 R6).
//!
//! It reads the pinned Unicode 17.0.0 inputs of [F20 §3.2] from `fixtures/ucd/17.0.0/` — `UnicodeData.txt` (field 3,
//! `Canonical_Combining_Class`; field 5, the canonical decomposition mappings) and `CaseFolding.txt` (the lines with
//! status `C` or `F`) — and writes the committed Rust module `crates/moirai-files/src/fold/tables.rs`, which
//! `moirai-files`' `fold` module uses to compute `fold_v1(x) = NFD(CF(NFD(x)))` ([F20 §3.1]).
//!
//! What the tables hold, per code point below a limit (every code point at or above it has class 0, no decomposition
//! and no folding):
//! - the canonical combining class;
//! - the **full** canonical decomposition: field 5 mappings without a `<tag>` (compatibility mappings are never
//!   applied), applied recursively; not canonically ordered, because the runtime orders the whole stream. Hangul
//!   syllables U+AC00–U+D7A3 are not in the tables: [F20 §3.1] decomposes them arithmetically;
//! - the full case folding: the mapping of the `C` or `F` line, verbatim (`S` and `T` lines are ignored).
//!
//! The lookup is a two-stage table: `STAGE1[cp >> SHIFT]` names a block of `1 << SHIFT` entries in `STAGE2`, whose
//! entry is an index into `RECORDS` (0: nothing). Identical blocks, records and mappings are stored once. `SHIFT` is
//! the value in 4..=9 that minimises the tables' bytes.
//!
//! Before it parses an input, the generator refuses one whose size differs from the pin of
//! `fixtures/ucd/17.0.0/INDEX.md` (a CRLF conversion, a truncated download or another version changes it), and a
//! `CaseFolding.txt` whose first line names another Unicode version ([`read_inputs`]). It then checks the inputs as it
//! parses them (field counts, ascending code points, paired ranges, scalar targets, at most one `C` or `F` line per
//! code point) and refuses anything it does not expect. The SHA-256 digests of `INDEX.md` are verified by
//! `moirai-files`' test `tests/fold_ucd.rs`, which also checks the generated tables against the files over every
//! scalar value. Output is deterministic; `--check` compares it with the committed module and changes nothing, and
//! this module's test `the_committed_tables_are_the_generators_output` runs the same comparison in every test run.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

/// The pinned inputs, relative to the repository root ([F20 §3.2]).
pub const UCD_DIR: &str = "fixtures/ucd/17.0.0";
/// The Unicode version of [`UCD_DIR`] ([F20 §3.1]).
pub const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);
/// The generated module, relative to the repository root.
pub const OUTPUT: &str = "crates/moirai-files/src/fold/tables.rs";
/// The pin table of [`UCD_DIR`] ([F20 §3.2]).
const INDEX: &str = "INDEX.md";

/// The Hangul syllable block, decomposed arithmetically ([F20 §3.1]); never in the tables.
const HANGUL_FIRST: u32 = 0xAC00;
const HANGUL_LAST: u32 = 0xD7A3;
/// The Hangul decomposition constants of [F20 §3.1] (the Unicode Standard §3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = 588;
/// Recursion bound of the canonical decomposition (the real depth is at most 3; a deeper chain is a cycle).
const MAX_DEPTH: u32 = 16;
/// The line width of the generated arrays.
const WIDTH: usize = 100;

/// The parsed properties, keyed by code point.
#[derive(Debug, Default)]
pub struct Ucd {
    /// `Canonical_Combining_Class` of every code point whose class is not 0.
    pub ccc: BTreeMap<u32, u8>,
    /// One level of canonical decomposition: UnicodeData.txt field 5 without a `<tag>`.
    pub decomp: BTreeMap<u32, Vec<u32>>,
    /// Full case folding: the mappings of the `C` and `F` lines of CaseFolding.txt.
    pub fold: BTreeMap<u32, Vec<u32>>,
}

/// A code point: 4 to 6 upper-case hexadecimal digits, at most U+10FFFF.
fn code_point(s: &str) -> Result<u32, String> {
    let ok =
        (4..=6).contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'));
    let v = if ok {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    };
    v.filter(|&v| v <= 0x10FFFF)
        .ok_or_else(|| format!("'{s}' is not a code point"))
}

/// A scalar value: a code point that is not a surrogate.
fn scalar(s: &str) -> Result<u32, String> {
    let v = code_point(s)?;
    if (0xD800..=0xDFFF).contains(&v) {
        return Err(format!("'{s}' is a surrogate, not a scalar value"));
    }
    Ok(v)
}

/// Parses `UnicodeData.txt` into `ucd.ccc` and `ucd.decomp`.
pub fn parse_unicode_data(text: &str, ucd: &mut Ucd) -> Result<(), String> {
    let mut prev: Option<u32> = None;
    let mut open_range: Option<(u32, &str)> = None;
    for (i, line) in text.lines().enumerate() {
        let at = |e: String| format!("UnicodeData.txt:{}: {e}", i + 1);
        let f: Vec<&str> = line.split(';').collect();
        if f.len() != 15 {
            return Err(at(format!("{} fields, expected 15", f.len())));
        }
        let cp = code_point(f[0]).map_err(at)?;
        if prev.is_some_and(|p| cp <= p) {
            return Err(at(format!(
                "U+{cp:04X} is not above the previous line's code point"
            )));
        }
        prev = Some(cp);
        let ccc: u8 = f[3]
            .parse()
            .map_err(|_| at(format!("class '{}' is not a number 0-255", f[3])))?;
        let name = f[1];
        let first = name.strip_suffix(", First>");
        let last = name.strip_suffix(", Last>");
        if first.is_some() || last.is_some() {
            // A range stands for every code point between its two lines. Every range of the UCD has class 0 and no
            // decomposition, so it adds nothing to the tables; one that did would need expanding, and is refused.
            if ccc != 0 || !f[5].is_empty() {
                return Err(at(format!(
                    "the range line {name} has a class or a decomposition"
                )));
            }
            match (first, last, open_range.take()) {
                (Some(r), None, None) => open_range = Some((cp, r)),
                (None, Some(r), Some((_, open))) if r == open => {}
                _ => return Err(at(format!("unpaired range line {name}"))),
            }
            continue;
        }
        if let Some((start, r)) = open_range {
            return Err(at(format!(
                "the range {r}> opened at U+{start:04X} has no Last line"
            )));
        }
        if ccc != 0 {
            ucd.ccc.insert(cp, ccc);
        }
        if !f[5].is_empty() && !f[5].starts_with('<') {
            if (0xD800..=0xDFFF).contains(&cp) {
                return Err(at(format!("surrogate U+{cp:04X} has a decomposition")));
            }
            if (HANGUL_FIRST..=HANGUL_LAST).contains(&cp) {
                return Err(at(format!(
                    "Hangul syllable U+{cp:04X} has a listed decomposition; [F20 §3.1] decomposes it arithmetically"
                )));
            }
            let m = f[5]
                .split(' ')
                .map(scalar)
                .collect::<Result<Vec<u32>, String>>()
                .map_err(at)?;
            // Canonical mappings have one or two code points (Unicode Standard §3.7, D63 and the UCD stability policy).
            if !(1..=2).contains(&m.len()) {
                return Err(at(format!(
                    "canonical mapping of U+{cp:04X} has {} code points",
                    m.len()
                )));
            }
            ucd.decomp.insert(cp, m);
        }
    }
    if let Some((start, r)) = open_range {
        return Err(format!(
            "UnicodeData.txt: the range {r}> opened at U+{start:04X} has no Last line"
        ));
    }
    Ok(())
}

/// Parses `CaseFolding.txt` into `ucd.fold`: the `C` and `F` lines; `S` and `T` lines are skipped.
pub fn parse_case_folding(text: &str, ucd: &mut Ucd) -> Result<(), String> {
    for (i, line) in text.lines().enumerate() {
        let at = |e: String| format!("CaseFolding.txt:{}: {e}", i + 1);
        let data = line.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        let f: Vec<&str> = data.split(';').map(str::trim).collect();
        // `<code>; <status>; <mapping>;` — the trailing `;` leaves an empty fourth field.
        if f.len() != 4 || !f[3].is_empty() {
            return Err(at(format!(
                "'{data}' is not '<code>; <status>; <mapping>;'"
            )));
        }
        let cp = scalar(f[0]).map_err(at)?;
        match f[1] {
            "C" | "F" => {
                let m = f[2]
                    .split(' ')
                    .map(scalar)
                    .collect::<Result<Vec<u32>, String>>()
                    .map_err(at)?;
                if !(1..=3).contains(&m.len()) {
                    return Err(at(format!(
                        "the folding of U+{cp:04X} has {} code points",
                        m.len()
                    )));
                }
                if ucd.fold.insert(cp, m).is_some() {
                    return Err(at(format!("U+{cp:04X} has two C or F lines")));
                }
            }
            "S" | "T" => {}
            s => return Err(at(format!("unknown status '{s}'"))),
        }
    }
    Ok(())
}

/// Appends the full canonical decomposition of `cp` to `out`: the canonical mappings applied recursively, Hangul
/// syllables arithmetically; a code point without a mapping stands for itself.
fn full_decomposition(ucd: &Ucd, cp: u32, out: &mut Vec<u32>, depth: u32) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "the canonical decomposition through U+{cp:04X} does not terminate"
        ));
    }
    if (HANGUL_FIRST..=HANGUL_LAST).contains(&cp) {
        let s = cp - S_BASE;
        out.push(L_BASE + s / N_COUNT);
        out.push(V_BASE + (s % N_COUNT) / T_COUNT);
        if !s.is_multiple_of(T_COUNT) {
            out.push(T_BASE + s % T_COUNT);
        }
    } else if let Some(m) = ucd.decomp.get(&cp) {
        for &d in m {
            full_decomposition(ucd, d, out, depth + 1)?;
        }
    } else {
        out.push(cp);
    }
    Ok(())
}

/// One record: (class, decomposition length, decomposition offset, folding length, folding offset).
type Rec = (u8, u8, u16, u8, u16);

/// The generated tables, before rendering.
#[derive(Debug)]
pub struct Tables {
    pub shift: u32,
    pub limit: u32,
    pub stage1: Vec<u16>,
    pub stage2: Vec<u16>,
    records: Vec<Rec>,
    pub decomp: Vec<u32>,
    pub fold: Vec<u32>,
    /// The number of code points with a class, a decomposition and a folding, for the header.
    counts: (usize, usize, usize),
}

impl Tables {
    /// The stage-1 element width in bytes: 1 while the block ids fit in a byte.
    fn stage1_width(&self) -> usize {
        if self.stage2.len() >> self.shift <= 256 {
            1
        } else {
            2
        }
    }

    /// The tables' size in bytes as the generated statics hold them.
    pub fn bytes(&self) -> usize {
        self.stage1.len() * self.stage1_width()
            + self.stage2.len() * 2
            + self.records.len() * 8
            + (self.decomp.len() + self.fold.len()) * 4
    }

    /// The lookup of the generated module, for the generator's own tests: (class, decomposition, folding).
    #[cfg(test)]
    pub fn lookup(&self, cp: u32) -> (u8, &[u32], &[u32]) {
        let r = if cp >= self.limit {
            (0, 0, 0, 0, 0)
        } else {
            let block = usize::from(self.stage1[(cp >> self.shift) as usize]);
            let mask = (1u32 << self.shift) - 1;
            self.records[usize::from(self.stage2[(block << self.shift) | (cp & mask) as usize])]
        };
        let (ccc, dlen, doff, flen, foff) = r;
        let d = &self.decomp[usize::from(doff)..][..usize::from(dlen)];
        let f = &self.fold[usize::from(foff)..][..usize::from(flen)];
        (ccc, d, f)
    }
}

/// Stores `seq` once in `data` and returns its offset.
fn intern(
    seq: Vec<u32>,
    data: &mut Vec<u32>,
    seen: &mut HashMap<Vec<u32>, u16>,
    what: &str,
) -> Result<u16, String> {
    if let Some(&o) = seen.get(&seq) {
        return Ok(o);
    }
    let o = u16::try_from(data.len())
        .map_err(|_| format!("the {what} data exceeds 65,536 code points"))?;
    data.extend_from_slice(&seq);
    seen.insert(seq, o);
    Ok(o)
}

/// Builds the records and the two-stage table from the parsed properties.
pub fn build(ucd: &Ucd) -> Result<Tables, String> {
    let mut keys: Vec<u32> = ucd
        .ccc
        .keys()
        .chain(ucd.decomp.keys())
        .chain(ucd.fold.keys())
        .copied()
        .collect();
    keys.sort_unstable();
    keys.dedup();

    let mut decomp = Vec::new();
    let mut fold = Vec::new();
    let mut seen_d = HashMap::new();
    let mut seen_f = HashMap::new();
    let mut records: Vec<Rec> = vec![(0, 0, 0, 0, 0)];
    let mut rec_ids: HashMap<Rec, u16> = HashMap::from([((0, 0, 0, 0, 0), 0)]);
    let mut leaves: BTreeMap<u32, u16> = BTreeMap::new();
    for &cp in &keys {
        let ccc = ucd.ccc.get(&cp).copied().unwrap_or(0);
        let (dlen, doff) = match ucd.decomp.get(&cp) {
            Some(_) => {
                let mut full = Vec::new();
                full_decomposition(ucd, cp, &mut full, 0)?;
                let len = u8::try_from(full.len())
                    .map_err(|_| format!("U+{cp:04X}: decomposition too long"))?;
                (
                    len,
                    intern(full, &mut decomp, &mut seen_d, "decomposition")?,
                )
            }
            None => (0, 0),
        };
        let (flen, foff) = match ucd.fold.get(&cp) {
            // Lengths are 1..=3, checked by the parser.
            Some(m) => (
                m.len() as u8,
                intern(m.clone(), &mut fold, &mut seen_f, "folding")?,
            ),
            None => (0, 0),
        };
        let rec = (ccc, dlen, doff, flen, foff);
        let id = match rec_ids.get(&rec) {
            Some(&id) => id,
            None => {
                let id = u16::try_from(records.len())
                    .map_err(|_| "more than 65,536 records".to_string())?;
                records.push(rec);
                rec_ids.insert(rec, id);
                id
            }
        };
        if id != 0 {
            leaves.insert(cp, id);
        }
    }

    let max = leaves.keys().next_back().copied().unwrap_or(0);
    let mut best: Option<Tables> = None;
    for shift in 4..=9u32 {
        let b = 1u32 << shift;
        let limit = (max + 1).div_ceil(b) * b;
        let mut stage2: Vec<u16> = vec![0; b as usize];
        let mut blocks: HashMap<Vec<u16>, u16> = HashMap::from([(vec![0; b as usize], 0)]);
        let mut stage1 = Vec::with_capacity((limit / b) as usize);
        for blk in 0..limit / b {
            let v: Vec<u16> = (0..b)
                .map(|o| leaves.get(&(blk * b + o)).copied().unwrap_or(0))
                .collect();
            let n =
                u16::try_from(blocks.len()).map_err(|_| "more than 65,536 blocks".to_string())?;
            let id = *blocks.entry(v).or_insert_with_key(|v| {
                stage2.extend_from_slice(v);
                n
            });
            stage1.push(id);
        }
        let t = Tables {
            shift,
            limit,
            stage1,
            stage2,
            records: records.clone(),
            decomp: decomp.clone(),
            fold: fold.clone(),
            counts: (ucd.ccc.len(), ucd.decomp.len(), ucd.fold.len()),
        };
        if best.as_ref().is_none_or(|b| t.bytes() < b.bytes()) {
            best = Some(t);
        }
    }
    best.ok_or_else(|| "no table shape".to_string())
}

/// Appends `items` as the body of an array literal: 4-space indent, lines of at most [`WIDTH`] columns.
fn push_items(out: &mut String, items: impl Iterator<Item = String>) {
    let mut line = String::from("   ");
    for it in items {
        if line.len() + 1 + it.len() + 1 > WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = String::from("   ");
        }
        line.push(' ');
        line.push_str(&it);
        line.push(',');
    }
    if line.len() > 3 {
        out.push_str(&line);
        out.push('\n');
    }
}

fn char_lit(cp: u32) -> String {
    format!("'\\u{{{cp:x}}}'")
}

/// Renders the module text.
pub fn render(t: &Tables) -> String {
    let (major, minor, update) = UNICODE_VERSION;
    let (n_ccc, n_decomp, n_fold) = t.counts;
    let s1_ty = if t.stage1_width() == 1 { "u8" } else { "u16" };
    let mut o = String::with_capacity(t.bytes() * 4);
    let _ = write!(
        o,
        "\
//! The `fold_v1` tables at Unicode {major}.{minor}.{update} ([F20 §3.1]): canonical combining classes, full canonical
//! decompositions and full case folding (statuses C and F).
//!
//! @generated by `cargo xtask ucd` (xtask/src/ucd.rs) from `UnicodeData.txt` and `CaseFolding.txt` in
//! `{UCD_DIR}/`, which pins them by SHA-256 in its `INDEX.md`. Do not edit this file: regenerate it.
//! `cargo xtask ucd --check` compares it with the generator's output.
//!
//! Data derived from the Unicode Character Database, copyright © Unicode, Inc., under the Unicode License v3
//! (`LICENSES/Unicode-3.0.txt`; NOTICE).
//!
//! Contents: {n_ccc} code points with a non-zero class, {n_decomp} with a canonical decomposition mapping,
//! {n_fold} with a C or F folding; {records} records, {bytes} bytes of tables. Hangul syllables U+AC00–U+D7A3 are
//! not listed: they decompose arithmetically ([F20 §3.1]).
//!
//! Lookup of code point `cp`: at or above [`LIMIT`] nothing (class 0, no decomposition, no folding); below it the
//! record `RECORDS[STAGE2[(STAGE1[cp >> SHIFT] << SHIFT) | (cp & ((1 << SHIFT) - 1))]]`, record 0 being nothing.

/// The Unicode version of the data ([F20 §3.1]).
pub(super) const UNICODE_VERSION: (u8, u8, u8) = ({major}, {minor}, {update});

/// log2 of the length of a stage-2 block.
pub(super) const SHIFT: u32 = {shift};

/// Every code point at or above this has class 0, no decomposition and no folding.
pub(super) const LIMIT: u32 = 0x{limit:X};

/// Stage 1: the stage-2 block of each run of `1 << SHIFT` code points below [`LIMIT`].
#[rustfmt::skip]
pub(super) static STAGE1: [{s1_ty}; {n1}] = [
",
        records = t.records.len(),
        bytes = t.bytes(),
        shift = t.shift,
        limit = t.limit,
        n1 = t.stage1.len(),
    );
    push_items(&mut o, t.stage1.iter().map(u16::to_string));
    let _ = write!(
        o,
        "];

/// Stage 2: the record of each code point, block by block.
#[rustfmt::skip]
pub(super) static STAGE2: [u16; {n2}] = [
",
        n2 = t.stage2.len()
    );
    push_items(&mut o, t.stage2.iter().map(u16::to_string));
    let _ = write!(
        o,
        "];

/// The records: (canonical combining class, decomposition length, decomposition offset in [`DECOMP`], folding length,
/// folding offset in [`FOLD`]). Record 0: class 0, no decomposition, no folding.
#[rustfmt::skip]
pub(super) static RECORDS: [(u8, u8, u16, u8, u16); {nr}] = [
",
        nr = t.records.len()
    );
    push_items(
        &mut o,
        t.records
            .iter()
            .map(|(c, dl, d, fl, f)| format!("({c}, {dl}, {d}, {fl}, {f})")),
    );
    let _ = write!(
        o,
        "];

/// Full canonical decompositions: the mappings of UnicodeData.txt field 5 without a `<tag>`, applied recursively (not
/// canonically ordered; the caller orders the whole stream).
#[rustfmt::skip]
pub(super) static DECOMP: [char; {nd}] = [
",
        nd = t.decomp.len()
    );
    push_items(&mut o, t.decomp.iter().map(|&c| char_lit(c)));
    let _ = write!(
        o,
        "];

/// Full case foldings: the mappings of the `C` and `F` lines of CaseFolding.txt.
#[rustfmt::skip]
pub(super) static FOLD: [char; {nf}] = [
",
        nf = t.fold.len()
    );
    push_items(&mut o, t.fold.iter().map(|&c| char_lit(c)));
    o.push_str("];\n");
    o
}

/// Generates the module text from the two input texts.
pub fn generate(unicode_data: &str, case_folding: &str) -> Result<(String, Tables), String> {
    let mut ucd = Ucd::default();
    parse_unicode_data(unicode_data, &mut ucd)?;
    parse_case_folding(case_folding, &mut ucd)?;
    let t = build(&ucd)?;
    Ok((render(&t), t))
}

/// The size pins of `INDEX.md`'s table: file name → bytes. A row is `` | `<file>` | <bytes> | `<sha-256>` | <role> | ``;
/// every other line is prose. A file pinned twice, or a row whose name or size does not parse, is refused.
fn parse_size_pins(index: &str) -> Result<BTreeMap<&str, usize>, String> {
    let mut pins = BTreeMap::new();
    for (i, line) in index.lines().enumerate() {
        if !line.starts_with("| `") {
            continue;
        }
        let at = |e: String| format!("{UCD_DIR}/{INDEX}:{}: {e}", i + 1);
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(name), Some(bytes)) = (cells.get(1), cells.get(2)) else {
            return Err(at("a pin row needs a file and a size".to_string()));
        };
        let name = name
            .strip_prefix('`')
            .and_then(|n| n.strip_suffix('`'))
            .filter(|n| !n.is_empty())
            .ok_or_else(|| at(format!("'{name}' is not a `file name`")))?;
        let bytes: usize = bytes
            .parse()
            .map_err(|_| at(format!("'{bytes}' is not a size in bytes")))?;
        if pins.insert(name, bytes).is_some() {
            return Err(at(format!("{name} is pinned twice")));
        }
    }
    Ok(pins)
}

/// The first line `CaseFolding.txt` of [`UNICODE_VERSION`] starts with.
fn case_folding_header() -> String {
    let (major, minor, update) = UNICODE_VERSION;
    format!("# CaseFolding-{major}.{minor}.{update}.txt")
}

/// Refuses an input that is not the pinned file: its size differs from `INDEX.md`'s pin, or, for `CaseFolding.txt`,
/// its first line names another version. `UnicodeData.txt` has no header; its pinned size stands for its version.
fn verify_input(name: &str, text: &str, pins: &BTreeMap<&str, usize>) -> Result<(), String> {
    let pinned = pins
        .get(name)
        .ok_or_else(|| format!("{UCD_DIR}/{INDEX} pins no {name}"))?;
    if text.len() != *pinned {
        return Err(format!(
            "{UCD_DIR}/{name} has {} bytes, {INDEX} pins {pinned}: it is not the pinned file (a CRLF conversion, a \
             truncated download or another version)",
            text.len()
        ));
    }
    if name == "CaseFolding.txt" {
        let header = case_folding_header();
        if !text.starts_with(&header) {
            return Err(format!(
                "{UCD_DIR}/{name} does not start with '{header}': it is not the Unicode {}.{}.{} file",
                UNICODE_VERSION.0, UNICODE_VERSION.1, UNICODE_VERSION.2
            ));
        }
    }
    Ok(())
}

/// Reads `UnicodeData.txt` and `CaseFolding.txt` from [`UCD_DIR`] under `repo` and verifies each against the pins of
/// `INDEX.md` ([`verify_input`]).
fn read_inputs(repo: &Path) -> Result<(String, String), String> {
    let read = |name: &str| {
        let p = repo.join(UCD_DIR).join(name);
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
    };
    let index = read(INDEX)?;
    let pins = parse_size_pins(&index)?;
    let unicode_data = read("UnicodeData.txt")?;
    verify_input("UnicodeData.txt", &unicode_data, &pins)?;
    let case_folding = read("CaseFolding.txt")?;
    verify_input("CaseFolding.txt", &case_folding, &pins)?;
    Ok((unicode_data, case_folding))
}

/// `cargo xtask ucd [--check]`. Returns false when `--check` finds the committed module out of date.
pub fn run(repo: &Path, check: bool) -> Result<bool, String> {
    let (unicode_data, case_folding) = read_inputs(repo)?;
    let (text, t) = generate(&unicode_data, &case_folding)?;
    let out = repo.join(OUTPUT);
    let old = std::fs::read_to_string(&out).ok();
    let summary = format!(
        "{} records, {} decomposition and {} folding code points, SHIFT {}, {} bytes",
        t.records.len(),
        t.decomp.len(),
        t.fold.len(),
        t.shift,
        t.bytes()
    );
    if check {
        if old.as_deref() == Some(text.as_str()) {
            println!("ucd: {OUTPUT} is up to date ({summary})");
            return Ok(true);
        }
        println!("ucd: {OUTPUT} differs from the generator's output; run `cargo xtask ucd`");
        return Ok(false);
    }
    if old.as_deref() == Some(text.as_str()) {
        println!("ucd: {OUTPUT} unchanged ({summary})");
    } else {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&out, &text).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("ucd: wrote {OUTPUT} ({summary})");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic UnicodeData.txt lines (15 fields).
    fn ud(lines: &[(&str, &str, &str, &str)]) -> String {
        lines
            .iter()
            .map(|(cp, name, ccc, dm)| format!("{cp};{name};Lu;{ccc};L;{dm};;;;N;;;;;\n"))
            .collect()
    }

    #[test]
    fn unicode_data_keeps_classes_and_canonical_mappings_only() {
        let text = ud(&[
            ("0041", "A", "0", ""),
            ("00C0", "A GRAVE", "0", "0041 0300"),
            ("00C5", "A RING", "0", "0041 030A"),
            ("01C4", "DZ CARON", "0", "<compat> 0044 017D"),
            ("0300", "GRAVE", "230", ""),
            ("030A", "RING", "230", ""),
            ("212B", "ANGSTROM", "0", "00C5"),
            ("3400", "<CJK Ideograph Extension A, First>", "0", ""),
            ("4DBF", "<CJK Ideograph Extension A, Last>", "0", ""),
        ]);
        let mut u = Ucd::default();
        parse_unicode_data(&text, &mut u).unwrap();
        assert_eq!(u.ccc, BTreeMap::from([(0x300, 230), (0x30A, 230)]));
        assert_eq!(
            u.decomp,
            BTreeMap::from([
                (0xC0, vec![0x41, 0x300]),
                (0xC5, vec![0x41, 0x30A]),
                (0x212B, vec![0xC5]),
            ])
        );
        let mut full = Vec::new();
        full_decomposition(&u, 0x212B, &mut full, 0).unwrap();
        assert_eq!(full, [0x41, 0x30A], "recursive");
        full.clear();
        full_decomposition(&u, 0xD55C, &mut full, 0).unwrap();
        assert_eq!(
            full,
            [0x1112, 0x1161, 0x11AB],
            "Hangul LVT, [F20 §3.1] example"
        );
        full.clear();
        full_decomposition(&u, 0xAC00, &mut full, 0).unwrap();
        assert_eq!(full, [0x1100, 0x1161], "Hangul LV");
    }

    #[test]
    fn unicode_data_refusals() {
        let bad = [
            ud(&[("0042", "B", "0", ""), ("0041", "A", "0", "")]),
            ud(&[("0041", "A", "256", "")]),
            ud(&[("0041", "A", "0", "D800")]),
            ud(&[("0041", "A", "0", "0042 0043 0044")]),
            ud(&[("AC01", "HANGUL", "0", "1100 1161")]),
            ud(&[("3400", "<X, First>", "0", "")]),
            ud(&[
                ("3400", "<X, First>", "0", ""),
                ("4DBF", "<Y, Last>", "0", ""),
            ]),
            ud(&[("3400", "<X, First>", "0", ""), ("3401", "Z", "0", "")]),
            ud(&[("4DBF", "<X, Last>", "0", "")]),
            ud(&[
                ("3400", "<X, First>", "230", ""),
                ("4DBF", "<X, Last>", "230", ""),
            ]),
            "0041;A;Lu;0\n".to_string(),
            ud(&[("41", "A", "0", "")]),
            ud(&[("110000", "A", "0", "")]),
            ud(&[("00c0", "A", "0", "")]),
        ];
        for t in bad {
            assert!(
                parse_unicode_data(&t, &mut Ucd::default()).is_err(),
                "accepted: {t}"
            );
        }
    }

    #[test]
    fn a_decomposition_cycle_is_refused() {
        let text = ud(&[("0100", "X", "0", "0101"), ("0101", "Y", "0", "0100")]);
        let mut u = Ucd::default();
        parse_unicode_data(&text, &mut u).unwrap();
        assert!(build(&u).is_err());
    }

    #[test]
    fn case_folding_keeps_c_and_f_only() {
        let text = "# CaseFolding-17.0.0.txt\n\n0041; C; 0061; # LATIN CAPITAL LETTER A\n\
                    00DF; F; 0073 0073; # LATIN SMALL LETTER SHARP S\n1E9E; F; 0073 0073; # CAPITAL SHARP S\n\
                    1E9E; S; 00DF; # CAPITAL SHARP S\n0049; T; 0131; # LATIN CAPITAL LETTER I\n";
        let mut u = Ucd::default();
        parse_case_folding(text, &mut u).unwrap();
        assert_eq!(
            u.fold,
            BTreeMap::from([
                (0x41, vec![0x61]),
                (0xDF, vec![0x73, 0x73]),
                (0x1E9E, vec![0x73, 0x73]),
            ])
        );
        for bad in [
            "0041; C; 0061; #\n0041; F; 0061 0061; #\n",
            "0041; X; 0061; #\n",
            "0041; C; 0061 0062 0063 0064; #\n",
            "0041; C; D800; #\n",
            "0041; C; 0061 #\n",
            "0041; C; 0061; extra; #\n",
        ] {
            assert!(
                parse_case_folding(bad, &mut Ucd::default()).is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn the_table_returns_every_property_and_nothing_else() {
        let mut u = Ucd::default();
        // Scattered properties, including one in the last block and shared mappings.
        for cp in (0x300..0x370).step_by(3) {
            u.ccc.insert(cp, (cp % 200) as u8 + 1);
        }
        u.ccc.insert(0x1E94A, 7);
        u.decomp.insert(0xC0, vec![0x41, 0x300]);
        u.decomp.insert(0xC1, vec![0x41, 0x301]);
        u.decomp.insert(0x212B, vec![0xC5]);
        u.decomp.insert(0xC5, vec![0x41, 0x30A]);
        u.decomp.insert(0x2FA1D, vec![0x2A600]);
        u.fold.insert(0x41, vec![0x61]);
        u.fold.insert(0xC0, vec![0xE0]);
        u.fold.insert(0x1E9E, vec![0x73, 0x73]);
        u.fold.insert(0xDF, vec![0x73, 0x73]);
        let t = build(&u).unwrap();
        assert_eq!(
            t.fold,
            [0x61, 0xE0, 0x73, 0x73],
            "shared foldings are stored once"
        );
        for cp in 0..0x110000u32 {
            let (ccc, d, f) = t.lookup(cp);
            assert_eq!(ccc, u.ccc.get(&cp).copied().unwrap_or(0), "U+{cp:04X}");
            let mut want = Vec::new();
            if u.decomp.contains_key(&cp) {
                full_decomposition(&u, cp, &mut want, 0).unwrap();
            }
            assert_eq!(d, want.as_slice(), "U+{cp:04X}");
            assert_eq!(
                f,
                u.fold.get(&cp).map_or(&[][..], Vec::as_slice),
                "U+{cp:04X}"
            );
        }
        assert!(t.limit > 0x2FA1D && t.limit.is_multiple_of(1 << t.shift));
    }

    #[test]
    fn rendering_is_deterministic_and_well_formed() {
        let text = ud(&[
            ("00C0", "A GRAVE", "0", "0041 0300"),
            ("0300", "GRAVE", "230", ""),
        ]);
        let (a, t) = generate(&text, "00C0; C; 00E0; # A GRAVE\n").unwrap();
        let (b, _) = generate(&text, "00C0; C; 00E0; # A GRAVE\n").unwrap();
        assert_eq!(a, b);
        assert!(
            a.contains("pub(super) static DECOMP: [char; 2] = [\n    '\\u{41}', '\\u{300}',\n];")
        );
        assert!(a.contains("pub(super) static FOLD: [char; 1] = [\n    '\\u{e0}',\n];"));
        assert!(a.contains("pub(super) const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);"));
        assert!(
            a.lines().all(|l| l.chars().count() <= WIDTH + 20),
            "line width"
        );
        assert_eq!(t.records.len(), 3, "nothing, U+00C0, U+0300");
    }

    #[test]
    fn inputs_that_differ_from_their_pins_are_refused() {
        let index = "# Unicode Character Database\n\nprose with `code` | and bars\n\n\
                     | File | Bytes | SHA-256 | Role |\n|---|---|---|---|\n\
                     | `UnicodeData.txt` | 12 | `00` | normative |\n\
                     | `CaseFolding.txt` | 29 | `00` | normative |\n";
        let pins = parse_size_pins(index).unwrap();
        assert_eq!(
            pins,
            BTreeMap::from([("CaseFolding.txt", 29), ("UnicodeData.txt", 12)])
        );
        let cf = "# CaseFolding-17.0.0.txt\n0041";
        assert_eq!(cf.len(), 29);
        verify_input("CaseFolding.txt", cf, &pins).unwrap();
        verify_input("UnicodeData.txt", "0041;A;Lu;0\n", &pins).unwrap();
        // A mismatching size: a CRLF conversion of the same text, and a truncated file.
        let crlf = "0041;A;Lu;0\r\n";
        let e = verify_input("UnicodeData.txt", crlf, &pins).unwrap_err();
        assert!(e.contains("13 bytes") && e.contains("pins 12"), "{e}");
        assert!(verify_input("UnicodeData.txt", "0041;A;Lu;", &pins).is_err());
        // The pinned size but another version's header.
        let other = "# CaseFolding-16.0.0.txt\n0041";
        assert_eq!(other.len(), 29);
        let e = verify_input("CaseFolding.txt", other, &pins).unwrap_err();
        assert!(e.contains("# CaseFolding-17.0.0.txt"), "{e}");
        // A file without a pin.
        assert!(verify_input("DerivedAge.txt", "", &pins).is_err());
        // Malformed tables.
        for bad in [
            "| `UnicodeData.txt` | 12 | `00` |\n| `UnicodeData.txt` | 12 | `00` |\n",
            "| `UnicodeData.txt` | twelve | `00` |\n",
            "| `UnicodeData.txt` | -12 | `00` |\n",
            "| `` | 12 | `00` |\n",
            "| `UnicodeData.txt\n",
        ] {
            assert!(parse_size_pins(bad).is_err(), "accepted: {bad}");
        }
    }

    /// `cargo xtask ucd --check` as a test: the pinned inputs pass [`read_inputs`], and the generator's output over
    /// them is the committed module byte for byte. A change to the generator that changes its output, or a hand edit
    /// of the module, fails here.
    #[test]
    fn the_committed_tables_are_the_generators_output() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let (unicode_data, case_folding) = read_inputs(&repo).unwrap();
        let (text, t) = generate(&unicode_data, &case_folding).unwrap();
        let committed = std::fs::read_to_string(repo.join(OUTPUT)).unwrap();
        assert!(
            committed == text,
            "{OUTPUT} is not the generator's output; run `cargo xtask ucd`"
        );
        assert_eq!(t.counts.2, 1_585, "C and F lines of CaseFolding-17.0.0.txt");
    }

    #[test]
    fn items_wrap_at_the_width() {
        let mut o = String::new();
        push_items(&mut o, (0..100).map(|i| format!("{i:05}")));
        assert!(
            o.lines()
                .all(|l| l.len() <= WIDTH && l.starts_with("    ") && l.ends_with(','))
        );
        assert_eq!(o.matches(',').count(), 100);
    }
}
