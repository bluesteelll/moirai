//! The Unicode 17.0.0 inputs of `fold_v1` ([F20 §3.1], §3.2), read at run time from `fixtures/ucd/17.0.0/` and
//! parsed by the model's own reader: `UnicodeData.txt` fields 3 (`Canonical_Combining_Class`) and 5 (canonical
//! decomposition mappings; a `<tag>` mapping is a compatibility mapping and is never kept), and the `C` and `F` lines of
//! `CaseFolding.txt`. Nothing is generated: the maps hold the files' own lines, and [`crate::r4::fold`] applies them by
//! definition (`docs/m0/PLAN.md` §3.2 item 9, §6.2 R5).
//!
//! Before anything is parsed, every file's size and SHA-256 are checked against the pins of `fixtures/ucd/17.0.0/INDEX.md`;
//! a file that differs is refused, since another file would be another Unicode version and another fold ([F20 §3.1]).
//! The cost of the load is recorded in [`UcdCost`] (WP-92: "record its cost").

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

/// The directory of the pinned UCD files.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ucd/17.0.0")
}

/// What loading the UCD cost: bytes read, wall time, and the number of entries the three maps hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct UcdCost {
    /// Bytes of `UnicodeData.txt` and `CaseFolding.txt` read and hashed.
    pub bytes: usize,
    /// Nanoseconds spent reading the files and checking their pins.
    pub read_ns: u128,
    /// Nanoseconds spent parsing them.
    pub parse_ns: u128,
    /// Canonical decomposition mappings kept.
    pub decompositions: usize,
    /// Code points with a non-zero combining class.
    pub combining: usize,
    /// `C` and `F` folding lines kept.
    pub foldings: usize,
    /// An upper bound of the heap the three maps hold, in bytes (entries and their mappings; the hash tables' own
    /// buckets are counted at their capacity).
    pub heap_bytes: usize,
}

/// The parsed inputs.
#[derive(Debug)]
pub struct Ucd {
    /// Canonical decomposition mappings, one level, by code point.
    decomp: HashMap<u32, Box<[u32]>>,
    /// Canonical combining classes other than 0.
    ccc: HashMap<u32, u8>,
    /// Full case folding: the mapping of the `C` or `F` line of a code point, with whether the line is `C`.
    fold: HashMap<u32, (bool, Box<[u32]>)>,
    /// The load's cost.
    pub cost: UcdCost,
}

impl Ucd {
    /// The canonical decomposition mapping of `c` (one level), if `UnicodeData.txt` gives one.
    pub fn decomposition(&self, c: u32) -> Option<&[u32]> {
        self.decomp.get(&c).map(|b| &b[..])
    }

    /// `Canonical_Combining_Class` of `c`; 0 for every code point the file does not list with another class.
    pub fn ccc(&self, c: u32) -> u8 {
        self.ccc.get(&c).copied().unwrap_or(0)
    }

    /// The full case folding of `c` (its `C` or `F` line), if any.
    pub fn folding(&self, c: u32) -> Option<&[u32]> {
        self.fold.get(&c).map(|b| &b.1[..])
    }

    /// The common case folding of `c` (its `C` line, a one-to-one mapping), if any: the per-character equivalence the
    /// simulated case-insensitive directory applies ([`crate::r4::tree`]).
    pub fn common_folding(&self, c: u32) -> Option<u32> {
        self.fold
            .get(&c)
            .filter(|b| b.0 && b.1.len() == 1)
            .map(|b| b.1[0])
    }
}

/// The pins of `INDEX.md`: (file name, size, SHA-256 hex).
fn pins() -> Vec<(String, usize, String)> {
    let p = dir().join("INDEX.md");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut out = Vec::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // | `UnicodeData.txt` | 2198209 | `2e1e…` | role |
        if cells.len() >= 5
            && cells[1].starts_with('`')
            && cells[1].ends_with(".txt`")
            && let Ok(size) = cells[2].parse::<usize>()
        {
            out.push((
                cells[1].trim_matches('`').to_string(),
                size,
                cells[3].trim_matches('`').to_string(),
            ));
        }
    }
    out
}

/// Reads one pinned file, refusing a size or digest that differs from its pin.
pub fn read_pinned(name: &str) -> Vec<u8> {
    let pin = pins()
        .into_iter()
        .find(|p| p.0 == name)
        .unwrap_or_else(|| panic!("fixtures/ucd/17.0.0/INDEX.md pins no {name}"));
    let p = dir().join(name);
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert_eq!(
        bytes.len(),
        pin.1,
        "{name}: {} bytes, the pin says {}",
        bytes.len(),
        pin.1
    );
    let got = crate::value::hex(&Sha256::digest(&bytes));
    assert_eq!(got, pin.2, "{name}: SHA-256 {got}, the pin says {}", pin.2);
    bytes
}

fn hex_cp(s: &str) -> u32 {
    u32::from_str_radix(s.trim(), 16).unwrap_or_else(|_| panic!("bad code point {s:?}"))
}

fn cps(s: &str) -> Box<[u32]> {
    s.split_ascii_whitespace().map(hex_cp).collect()
}

/// Parses `UnicodeData.txt` (fields 3 and 5) and `CaseFolding.txt` (statuses `C` and `F`).
pub fn parse(unicode_data: &str, case_folding: &str) -> (Ucd, UcdCost) {
    let mut decomp = HashMap::new();
    let mut ccc = HashMap::new();
    for line in unicode_data.lines() {
        let f: Vec<&str> = line.split(';').collect();
        if f.len() < 6 {
            continue;
        }
        let c = hex_cp(f[0]);
        let class: u8 = f[3]
            .parse()
            .unwrap_or_else(|_| panic!("bad combining class in {line:?}"));
        if class != 0 {
            ccc.insert(c, class);
        }
        let d = f[5].trim();
        // A `<tag>` mapping is a compatibility mapping: never applied ([F20 §3.1]).
        if !d.is_empty() && !d.starts_with('<') {
            decomp.insert(c, cps(d));
        }
    }
    let mut fold = HashMap::new();
    for line in case_folding.lines() {
        let body = line.split('#').next().unwrap_or("");
        let f: Vec<&str> = body.split(';').map(str::trim).collect();
        if f.len() < 3 || f[0].is_empty() {
            continue;
        }
        if f[1] == "C" || f[1] == "F" {
            let prev = fold.insert(hex_cp(f[0]), (f[1] == "C", cps(f[2])));
            assert!(prev.is_none(), "two C or F lines for {}", f[0]);
        }
    }
    let heap_bytes = decomp.capacity() * (4 + 16)
        + decomp.values().map(|v| v.len() * 4).sum::<usize>()
        + ccc.capacity() * 5
        + fold.capacity() * (4 + 16)
        + fold.values().map(|v| v.1.len() * 4).sum::<usize>();
    let cost = UcdCost {
        decompositions: decomp.len(),
        combining: ccc.len(),
        foldings: fold.len(),
        heap_bytes,
        ..UcdCost::default()
    };
    (
        Ucd {
            decomp,
            ccc,
            fold,
            cost,
        },
        cost,
    )
}

/// The UCD, loaded once per process from the pinned files.
pub fn ucd() -> &'static Ucd {
    static U: OnceLock<Ucd> = OnceLock::new();
    U.get_or_init(|| {
        let t0 = Instant::now();
        let ud = read_pinned("UnicodeData.txt");
        let cf = read_pinned("CaseFolding.txt");
        let read_ns = t0.elapsed().as_nanos();
        let t1 = Instant::now();
        let ud = String::from_utf8(ud).expect("UnicodeData.txt is UTF-8");
        let cf = String::from_utf8(cf).expect("CaseFolding.txt is UTF-8");
        assert!(
            cf.lines()
                .next()
                .is_some_and(|l| l.contains("CaseFolding-17.0.0")),
            "CaseFolding.txt names another version"
        );
        let (mut u, _) = parse(&ud, &cf);
        u.cost.bytes = ud.len() + cf.len();
        u.cost.read_ns = read_ns;
        u.cost.parse_ns = t1.elapsed().as_nanos();
        u
    })
}
