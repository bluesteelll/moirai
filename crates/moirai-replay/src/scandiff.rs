//! The scope-scanner differential of [F21 §3.9] ([40 §8.3.4] row 8; WP-63's acceptance, WP-74, WP-76): FL-1's Rust
//! scanner ([`moirai_files::scan`]) against the items `moirai-tsoracle` vouches for.
//!
//! For one file, the scanner reads the anchor text t = `atext(b)` of the raw bytes b ([F21 §1.3]) and the oracle reads
//! b itself (it skips one BOM, counts lines at `0A` and spells a CR LF inside a literal as it spells an LF, so its
//! lines and names are those of t). The two item lists are compared **as multisets** ([F21 §2.2]: a name path need not
//! name one item), at three levels, reported separately ([F21 §3.9] "The differential"):
//!
//! | Level | Key of an item |
//! |---|---|
//! | name path | its name path: the (`skind`, `name`, `qual`) triples of its ancestors and its own ([F21 §2.2]) |
//! | header line | the name path and `start` ([F20 §2.8]: the line whose text `span_hash` covers) |
//! | span | the name path, `start` and `end` (a `symbol` anchor's hint, [F20 §6.1] step 7) |
//!
//! The denominator is the oracle's **claimed** items (`ok`, its rule 8). At each level the agreement of a key is the
//! smaller of its claimed count and its scanner count. The claimed items the scanner lacks are **missing**; the scanner
//! items that no oracle item has, claimed or not, are **extra**. Unclaimed items are counted beside the rates, and so
//! are the files whose scan failed ([F21 §2.7]) or that are not text (no scanner runs on binary content, [F21 §1.3]):
//! their claimed items leave the denominator and are reported with their own counts, so no exclusion is silent.
//!
//! A name or qualifier longer than [`SCOPE_MAX_BYTES`] bytes is compared as a marker ([F21 §2.3]) holding the prefix
//! the scanner keeps of it ([`moirai_files::scan::Item::name`], [`LONG_PREFIX`]): no recordable name path holds one,
//! and two long texts agree iff their prefixes do, so a wrong spelling within its first bytes still shows.
//!
//! **What a report names.** A report lists every file that disagrees with its missing and extra items, except under
//! [`COUNTS_ONLY`]: there it gives the counts only. An item's name path and lines are read from its source, and every
//! role may read this crate and run its tests, roles that must not read those crates among them (`docs/m0/PLAN.md`
//! §3.1, S1, S2, S4; `xtask/roles.toml`): a failing report reaches them through a gate run on their branch or a CI log.
//! An author who may read every crate opts in to the names in their own work tree with [`NAME_ITEMS_ENV`]
//! ([`Report::named`]).

use std::collections::BTreeMap;
use std::fmt;

use moirai_files::scan::{Items, Lang, SCOPE_MAX_BYTES, scan};
use moirai_files::text::atext;

use crate::tsoracle::Record;

/// Path prefixes (repository-relative, `/` separators) whose items a [`Report`] counts but does not name unless asked
/// to ([`NAME_ITEMS_ENV`]): the union of the `deny_read` paths of `xtask/roles.toml` (`docs/m0/PLAN.md` §3.1). The
/// model, which no engine or harness role reads (S2); the format oracle, which R-FIX, R-HARN and the engine roles do
/// not read (S1); the toy log, which R-MODEL, R-FIX and R-ORA do not read and whose seeded-bug module the enumerator's
/// author does not read before WP-32 (S4); the product crates and the simulator, which R-MODEL does not read (S2) and
/// R-FIX and R-ORA do not read either (S1). R-FL1B runs this differential for WP-63's acceptance, and the gate prints a
/// failing test's report on any role's branch.
pub const COUNTS_ONLY: &[&str] = &[
    "crates/moirai-model/",
    "crates/moirai-format-oracle/",
    "crates/moirai-toylog/",
    "crates/moirai-vfs/",
    "crates/moirai-vfs-sim/",
    "crates/moirai-os/",
    "crates/moirai-files/",
    "crates/moirai-diff/",
];

/// The environment variable that, set to `1`, makes a test print its report with every disagreement named
/// ([`Report::named`]): for R-FL1B or R-REPLAY debugging in their own work trees. The gate and CI never set it.
pub const NAME_ITEMS_ENV: &str = "MOIRAI_REPLAY_NAME_ITEMS";

/// Whether a report gives only the counts of `path`'s disagreements ([`COUNTS_ONLY`]).
#[must_use]
pub fn counts_only(path: &str) -> bool {
    COUNTS_ONLY.iter().any(|p| path.starts_with(p))
}

/// Whether [`NAME_ITEMS_ENV`] is set to `1`.
#[must_use]
pub fn name_items_requested() -> bool {
    std::env::var_os(NAME_ITEMS_ENV).is_some_and(|v| v == "1")
}

/// The most bytes the scanner keeps of a long name or qualifier, cut at a character boundary
/// ([`moirai_files::scan::Item::name`]: "only its first bytes (at most 64)").
pub const LONG_PREFIX: usize = 64;

/// The longest prefix of `s` of at most [`LONG_PREFIX`] bytes that ends at a character boundary.
fn long_prefix(s: &str) -> &str {
    let mut k = s.len().min(LONG_PREFIX);
    while !s.is_char_boundary(k) {
        k -= 1;
    }
    &s[..k]
}

/// A name or qualifier as the differential compares it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Text {
    /// At most [`SCOPE_MAX_BYTES`] bytes, compared byte for byte.
    Full(String),
    /// Longer than [`SCOPE_MAX_BYTES`] bytes ([F21 §2.3]), held as its first bytes ([`LONG_PREFIX`]): equal to every
    /// long text with the same prefix, and to no short one.
    Long(String),
}

impl Text {
    /// The compared form of a whole name or qualifier.
    #[must_use]
    pub fn of(s: &str) -> Text {
        if s.len() > SCOPE_MAX_BYTES {
            Text::Long(long_prefix(s).to_string())
        } else {
            Text::Full(s.to_string())
        }
    }

    /// The compared form of what the scanner keeps of a name or qualifier: the whole text, or, when `long`, its
    /// prefix, cut again to [`LONG_PREFIX`] bytes (so a scanner that kept a longer prefix is compared, not refused).
    #[must_use]
    pub fn kept(s: &str, long: bool) -> Text {
        if long {
            Text::Long(long_prefix(s).to_string())
        } else {
            Text::of(s)
        }
    }

    fn is_empty(&self) -> bool {
        matches!(self, Text::Full(s) if s.is_empty())
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Text::Full(s) => f.write_str(s),
            // The mark the scanner's scope texts put after a long prefix (`Items::path_text`).
            Text::Long(p) => write!(f, "{p}%\u{2026}"),
        }
    }
}

/// One item in the shape both sides share: the fields of the oracle's record ([F21 §2.1]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Row {
    /// Rust `skind` 1 `mod` … 9 `macro_rules` ([F08 §10.3.1]).
    pub skind: u8,
    /// The name.
    pub name: Text,
    /// The qualifier; empty unless a trait `impl`.
    pub qual: Text,
    /// The header line.
    pub start: u64,
    /// The last line.
    pub end: u64,
    /// The index of the enclosing item.
    pub parent: Option<usize>,
}

impl Row {
    /// A row from whole names and qualifiers, for expected values.
    #[must_use]
    pub fn new(
        skind: u8,
        name: &str,
        qual: &str,
        start: u64,
        end: u64,
        parent: Option<usize>,
    ) -> Row {
        Row {
            skind,
            name: Text::of(name),
            qual: Text::of(qual),
            start,
            end,
            parent,
        }
    }
}

impl fmt::Display for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = Lang::Rust.skind_name(self.skind).unwrap_or("?");
        write!(f, "{kind} {}", self.name)?;
        if !self.qual.is_empty() {
            write!(f, "[{}]", self.qual)?;
        }
        write!(f, " {}–{}", self.start, self.end)?;
        if let Some(p) = self.parent {
            write!(f, " in #{p}")?;
        }
        Ok(())
    }
}

/// The rows of a scanner result, in pre-order.
#[must_use]
pub fn scanner_rows(items: &Items) -> Vec<Row> {
    items
        .iter()
        .map(|it| Row {
            skind: it.skind,
            name: Text::kept(it.name, it.name_long),
            qual: Text::kept(it.qual, it.qual_long),
            start: it.start,
            end: it.end,
            parent: it.parent,
        })
        .collect()
}

/// The rows of an oracle record with each item's `ok` flag, in pre-order.
#[must_use]
pub fn oracle_rows(rec: &Record) -> Vec<(Row, bool)> {
    rec.items
        .iter()
        .map(|it| {
            (
                Row {
                    skind: it.skind,
                    name: Text::of(&it.name),
                    qual: Text::of(&it.qual),
                    start: it.start,
                    end: it.end,
                    parent: it.parent,
                },
                it.ok,
            )
        })
        .collect()
}

/// A name path: the (`skind`, `name`, `qual`) triples from the outermost ancestor to the item ([F21 §2.2]).
pub type NamePath = Vec<(u8, Text, Text)>;

/// The name path of every row, from the parents (which precede their children).
#[must_use]
pub fn name_paths<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Vec<NamePath> {
    let mut out: Vec<NamePath> = Vec::new();
    for r in rows {
        let mut p = r
            .parent
            .and_then(|i| out.get(i))
            .cloned()
            .unwrap_or_default();
        p.push((r.skind, r.name.clone(), r.qual.clone()));
        out.push(p);
    }
    out
}

/// The scope text of a name path ([F14 §5.6]): `rust:mod a/impl S[Tr]/fn f`.
#[must_use]
pub fn path_text(path: &NamePath) -> String {
    let mut s = String::from("rust:");
    for (i, (k, name, qual)) in path.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(Lang::Rust.skind_name(*k).unwrap_or("?"));
        s.push(' ');
        s.push_str(&name.to_string());
        if !qual.is_empty() {
            s.push('[');
            s.push_str(&qual.to_string());
            s.push(']');
        }
    }
    s
}

/// The three comparison levels of [F21 §3.9].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// The name path.
    NamePath = 0,
    /// The name path and the header line.
    Header = 1,
    /// The name path, the header line and the last line.
    Span = 2,
}

impl Level {
    /// The three levels, in order.
    pub const ALL: [Level; 3] = [Level::NamePath, Level::Header, Level::Span];

    /// Its name in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Level::NamePath => "name path",
            Level::Header => "header line",
            Level::Span => "span",
        }
    }
}

/// What the scanner made of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanSide {
    /// Its items.
    Items(Vec<Row>),
    /// The scan failed ([F21 §2.7]).
    Failed,
    /// The content is not text, so no scanner runs ([F21 §1.3], [F20 §2.1]).
    NotText,
}

/// The scanner's side of the raw bytes of a Rust file: `scan(atext(b))`.
#[must_use]
pub fn scan_side(bytes: &[u8]) -> ScanSide {
    match atext(bytes) {
        None => ScanSide::NotText,
        Some(t) => match scan(Lang::Rust, &t) {
            Ok(items) => ScanSide::Items(scanner_rows(&items)),
            Err(_) => ScanSide::Failed,
        },
    }
}

/// The differential of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    /// The file.
    pub path: String,
    /// The oracle's `errors`.
    pub oracle_errors: u64,
    /// The scanner's outcome, without its rows.
    pub outcome: Outcome,
    /// The oracle's claimed items.
    pub claimed: u64,
    /// The oracle's unclaimed items.
    pub unclaimed: u64,
    /// The scanner's items.
    pub scanner: u64,
    /// Agreeing items per [`Level`].
    pub agree: [u64; 3],
    /// Claimed items whose name path holds a long text.
    pub long: u64,
    /// Claimed items the scanner lacks, at the span level: (the item, how many).
    pub missing: Vec<(String, u64)>,
    /// Scanner items no oracle item has, at the span level.
    pub extra: Vec<(String, u64)>,
}

/// The scanner's outcome for a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Items were compared.
    Compared,
    /// The scan failed.
    Failed,
    /// Not text.
    NotText,
}

impl FileDiff {
    /// Whether every claimed item agrees at every level and the scanner reports nothing the oracle lacks.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.claimed_agree() && self.extra.is_empty()
    }

    /// Whether every claimed item agrees at every level (the scanner may report items the oracle lacks). This is what
    /// the oracle vouches for in a file with syntax errors, where error recovery can drop an item from its tree
    /// ([F21 §3.9] "The oracle's claim").
    #[must_use]
    pub fn claimed_agree(&self) -> bool {
        self.outcome == Outcome::Compared
            && self.agree == [self.claimed; 3]
            && self.missing.is_empty()
    }
}

/// Compares the scanner's side of `path` with the oracle's record for it.
#[must_use]
pub fn compare(path: &str, side: &ScanSide, record: &Record) -> FileDiff {
    let oracle = oracle_rows(record);
    let claimed = oracle.iter().filter(|(_, ok)| *ok).count() as u64;
    let mut d = FileDiff {
        path: path.to_string(),
        oracle_errors: record.errors,
        outcome: Outcome::Compared,
        claimed,
        unclaimed: oracle.len() as u64 - claimed,
        scanner: 0,
        agree: [0; 3],
        long: 0,
        missing: Vec::new(),
        extra: Vec::new(),
    };
    let rows = match side {
        ScanSide::Items(rows) => rows,
        ScanSide::Failed => {
            d.outcome = Outcome::Failed;
            return d;
        }
        ScanSide::NotText => {
            d.outcome = Outcome::NotText;
            return d;
        }
    };
    d.scanner = rows.len() as u64;
    let o_paths = name_paths(oracle.iter().map(|(r, _)| r));
    let s_paths = name_paths(rows);
    d.long = oracle
        .iter()
        .zip(&o_paths)
        .filter(|((_, ok), p)| {
            *ok && p
                .iter()
                .any(|(_, n, q)| matches!(n, Text::Long(_)) || matches!(q, Text::Long(_)))
        })
        .count() as u64;

    type Key = (NamePath, u64, u64);
    let key = |p: &NamePath, r: &Row, level: Level| -> Key {
        match level {
            Level::NamePath => (p.clone(), 0, 0),
            Level::Header => (p.clone(), r.start, 0),
            Level::Span => (p.clone(), r.start, r.end),
        }
    };
    for level in Level::ALL {
        // Per key: claimed, unclaimed and scanner counts.
        let mut m: BTreeMap<Key, [u64; 3]> = BTreeMap::new();
        for ((r, ok), p) in oracle.iter().zip(&o_paths) {
            m.entry(key(p, r, level)).or_default()[usize::from(!*ok)] += 1;
        }
        for (r, p) in rows.iter().zip(&s_paths) {
            m.entry(key(p, r, level)).or_default()[2] += 1;
        }
        for (k, [ok, un, sc]) in &m {
            let agree = (*ok).min(*sc);
            d.agree[level as usize] += agree;
            if level == Level::Span {
                let show = |(p, s, e): &Key| format!("{} {s}–{e}", path_text(p));
                if *ok > agree {
                    d.missing.push((show(k), ok - agree));
                }
                if *sc > ok + un {
                    d.extra.push((show(k), sc - ok - un));
                }
            }
        }
    }
    d
}

/// The differential over a set of files: counts, rates and every disagreement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files compared (the scan did not fail and the content is text).
    pub files: u64,
    /// Compared files whose oracle record has `errors` > 0.
    pub with_errors: u64,
    /// Files whose scan failed, and the claimed items they hold.
    pub failed: (u64, u64),
    /// Files that are not text, and the claimed items they hold.
    pub not_text: (u64, u64),
    /// Claimed items of the compared files: the denominator.
    pub claimed: u64,
    /// Unclaimed items of the compared files.
    pub unclaimed: u64,
    /// Scanner items of the compared files.
    pub scanner: u64,
    /// Agreeing items per [`Level`].
    pub agree: [u64; 3],
    /// Claimed items whose name path holds a long text.
    pub long: u64,
    /// Every file that disagrees, with its missing and extra items.
    pub disagreements: Vec<FileDiff>,
}

impl Report {
    /// Adds one file.
    pub fn add(&mut self, d: FileDiff) {
        match d.outcome {
            Outcome::Failed => {
                self.failed.0 += 1;
                self.failed.1 += d.claimed;
                self.disagreements.push(d);
                return;
            }
            Outcome::NotText => {
                self.not_text.0 += 1;
                self.not_text.1 += d.claimed;
                self.disagreements.push(d);
                return;
            }
            Outcome::Compared => {}
        }
        self.files += 1;
        self.with_errors += u64::from(d.oracle_errors > 0);
        self.claimed += d.claimed;
        self.unclaimed += d.unclaimed;
        self.scanner += d.scanner;
        for (a, b) in self.agree.iter_mut().zip(d.agree) {
            *a += b;
        }
        self.long += d.long;
        if !d.agrees() {
            self.disagreements.push(d);
        }
    }

    /// The agreement rate at `level`, in percent; 100 when nothing is claimed.
    #[must_use]
    pub fn rate(&self, level: Level) -> f64 {
        if self.claimed == 0 {
            100.0
        } else {
            // Counts stay far below 2^53, so the conversions are exact.
            self.agree[level as usize] as f64 * 100.0 / self.claimed as f64
        }
    }

    /// Items the scanner reports that no oracle item has, over all compared files.
    #[must_use]
    pub fn extra(&self) -> u64 {
        self.disagreements
            .iter()
            .flat_map(|d| d.extra.iter().map(|(_, n)| n))
            .sum()
    }

    /// The report with every disagreement named, [`COUNTS_ONLY`] included: for an author who may read every crate,
    /// in their own work tree ([`NAME_ITEMS_ENV`]). Its `Display` form names none under [`COUNTS_ONLY`].
    #[must_use]
    pub fn named(&self) -> Named<'_> {
        Named(self)
    }

    /// The counts and the three agreement rates without the disagreements: what a run shows in its log whether or not
    /// libtest captures the test's output (the full report goes to the captured output and a file).
    #[must_use]
    pub fn summary(&self) -> Summary<'_> {
        Summary(self)
    }

    fn write_counts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "files: {} compared ({} with syntax errors), {} failed scans ({} claimed items), {} not text ({} claimed items)",
            self.files,
            self.with_errors,
            self.failed.0,
            self.failed.1,
            self.not_text.0,
            self.not_text.1
        )?;
        writeln!(
            f,
            "items: {} claimed by the oracle, {} unclaimed; {} from the scanner, {} of them extra; {} with a long name",
            self.claimed,
            self.unclaimed,
            self.scanner,
            self.extra(),
            self.long
        )?;
        for level in Level::ALL {
            writeln!(
                f,
                "{}: {} / {} = {:.3} %",
                level.name(),
                self.agree[level as usize],
                self.claimed,
                self.rate(level)
            )?;
        }
        Ok(())
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, name_all: bool) -> fmt::Result {
        self.write_counts(f)?;
        for d in &self.disagreements {
            match d.outcome {
                Outcome::Failed => writeln!(f, "{}: scan failed", d.path)?,
                Outcome::NotText => writeln!(f, "{}: not text", d.path)?,
                Outcome::Compared if !name_all && counts_only(&d.path) => {
                    let sum = |v: &[(String, u64)]| v.iter().map(|(_, n)| n).sum::<u64>();
                    writeln!(
                        f,
                        "{}: {} missing, {} extra (items not named: docs/m0/PLAN.md §3.1)",
                        d.path,
                        sum(&d.missing),
                        sum(&d.extra)
                    )?;
                }
                Outcome::Compared => {
                    writeln!(f, "{}:", d.path)?;
                    for (item, n) in &d.missing {
                        writeln!(f, "  missing ×{n}: {item}")?;
                    }
                    for (item, n) in &d.extra {
                        writeln!(f, "  extra ×{n}: {item}")?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The counts and rates, with every disagreement named except under [`COUNTS_ONLY`].
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, false)
    }
}

/// A [`Report`] displayed with every disagreement named ([`Report::named`]).
#[derive(Clone, Copy, Debug)]
pub struct Named<'a>(&'a Report);

impl fmt::Display for Named<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.write(f, true)
    }
}

/// A [`Report`]'s counts and rates alone ([`Report::summary`]): it names no file.
#[derive(Clone, Copy, Debug)]
pub struct Summary<'a>(&'a Report);

impl fmt::Display for Summary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.write_counts(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsoracle::OracleItem;

    fn item(
        skind: u8,
        name: &str,
        qual: &str,
        lines: (u64, u64),
        parent: Option<usize>,
        ok: bool,
    ) -> OracleItem {
        OracleItem {
            skind,
            name: name.into(),
            qual: qual.into(),
            start: lines.0,
            end: lines.1,
            parent,
            ok,
        }
    }

    fn record(items: Vec<OracleItem>) -> Record {
        Record {
            path: "x.rs".into(),
            errors: u64::from(items.iter().any(|i| !i.ok)),
            items,
        }
    }

    const SRC: &[u8] = b"mod a {\n    fn f() {}\n}\nimpl Display for S {}\n";

    #[test]
    fn the_scanner_and_a_matching_record_agree_at_every_level() {
        let rec = record(vec![
            item(1, "a", "", (1, 3), None, true),
            item(3, "f", "", (2, 2), Some(0), true),
            item(2, "S", "Display", (4, 4), None, true),
        ]);
        let d = compare("x.rs", &scan_side(SRC), &rec);
        assert!(d.agrees(), "{d:?}");
        assert_eq!(d.agree, [3, 3, 3]);
        let mut r = Report::default();
        r.add(d);
        assert_eq!(r.rate(Level::Span), 100.0);
        assert!(r.disagreements.is_empty());
    }

    #[test]
    fn levels_are_counted_separately() {
        // `f` with another end line, `S` with another header line, and an item the scanner does not report.
        let rec = record(vec![
            item(1, "a", "", (1, 3), None, true),
            item(3, "f", "", (2, 3), Some(0), true),
            item(2, "S", "Display", (5, 5), None, true),
            item(3, "g", "", (4, 4), None, true),
        ]);
        let d = compare("x.rs", &scan_side(SRC), &rec);
        assert_eq!(d.claimed, 4);
        assert_eq!(d.agree, [3, 2, 1]);
        // Keys are ordered by name path (`skind` first), then lines.
        assert_eq!(
            d.missing,
            [
                ("rust:mod a/fn f 2–3".to_string(), 1),
                ("rust:impl S[Display] 5–5".to_string(), 1),
                ("rust:fn g 4–4".to_string(), 1),
            ]
        );
        assert_eq!(
            d.extra,
            [
                ("rust:mod a/fn f 2–2".to_string(), 1),
                ("rust:impl S[Display] 4–4".to_string(), 1),
            ]
        );
        let mut r = Report::default();
        r.add(d);
        assert_eq!(r.extra(), 2);
        assert!((r.rate(Level::NamePath) - 75.0).abs() < 1e-9);
        let text = r.to_string();
        assert!(text.contains("name path: 3 / 4 = 75.000 %"), "{text}");
        assert!(text.contains("missing ×1: rust:fn g 4–4"), "{text}");
    }

    #[test]
    fn unclaimed_items_leave_the_denominator_but_excuse_the_scanner() {
        let rec = record(vec![
            item(1, "a", "", (1, 3), None, false),
            item(3, "f", "", (2, 2), Some(0), false),
            item(2, "S", "Display", (4, 4), None, true),
        ]);
        let d = compare("x.rs", &scan_side(SRC), &rec);
        assert_eq!((d.claimed, d.unclaimed, d.scanner), (1, 2, 3));
        assert!(d.agrees(), "{d:?}");
    }

    #[test]
    fn multisets_count_repeated_name_paths() {
        let src = b"#[cfg(a)]\nfn f() {}\n#[cfg(b)]\nfn f() {}\n";
        let twice = record(vec![
            item(3, "f", "", (2, 2), None, true),
            item(3, "f", "", (4, 4), None, true),
        ]);
        assert!(compare("x.rs", &scan_side(src), &twice).agrees());
        let once = record(vec![item(3, "f", "", (2, 2), None, true)]);
        let d = compare("x.rs", &scan_side(src), &once);
        assert_eq!(d.agree, [1, 1, 1]);
        assert_eq!(d.extra, [("rust:fn f 4–4".to_string(), 1)]);
    }

    #[test]
    fn failed_and_binary_files_are_counted_apart() {
        let deep = "{".repeat(1025);
        assert_eq!(scan_side(deep.as_bytes()), ScanSide::Failed);
        assert_eq!(scan_side(b"fn f() {}\0"), ScanSide::NotText);
        let rec = record(vec![item(3, "f", "", (1, 1), None, true)]);
        let mut r = Report::default();
        r.add(compare("deep.rs", &ScanSide::Failed, &rec));
        r.add(compare("bin.rs", &ScanSide::NotText, &rec));
        assert_eq!((r.files, r.claimed), (0, 0));
        assert_eq!(r.failed, (1, 1));
        assert_eq!(r.not_text, (1, 1));
        assert_eq!(r.disagreements.len(), 2);
        let text = r.to_string();
        assert!(
            text.contains("deep.rs: scan failed") && text.contains("bin.rs: not text"),
            "{text}"
        );
    }

    #[test]
    fn long_names_compare_as_markers_with_their_prefixes() {
        let long = "A".repeat(SCOPE_MAX_BYTES + 1);
        // A long qualifier too, whose first character is not ASCII: its prefix ends at a character boundary.
        let qual = format!("a{}", "é".repeat(SCOPE_MAX_BYTES / 2));
        let src = format!("impl {qual} for {long} {{}}\n");
        let side = scan_side(src.as_bytes());
        let ScanSide::Items(rows) = &side else {
            panic!("{side:?}")
        };
        assert_eq!(rows[0].name, Text::Long("A".repeat(LONG_PREFIX)));
        let cut = format!("a{}", "é".repeat(LONG_PREFIX / 2 - 1));
        assert_eq!(cut.len(), LONG_PREFIX - 1);
        assert_eq!(rows[0].qual, Text::Long(cut.clone()));
        assert_eq!(rows[0].qual.to_string(), format!("{cut}%\u{2026}"));
        let rec = record(vec![item(2, &long, &qual, (1, 1), None, true)]);
        let d = compare("x.rs", &side, &rec);
        assert!(d.agrees(), "{d:?}");
        assert_eq!(d.long, 1);
        // Long texts that differ only past the prefix agree; within it, they do not.
        let late = format!("{}B", &long[..SCOPE_MAX_BYTES]);
        let rec = record(vec![item(2, &late, &qual, (1, 1), None, true)]);
        assert!(compare("x.rs", &side, &rec).agrees());
        let early = format!("B{}", &long[1..]);
        let rec = record(vec![item(2, &early, &qual, (1, 1), None, true)]);
        let d = compare("x.rs", &side, &rec);
        assert_eq!((d.agree, d.missing.len(), d.extra.len()), ([0; 3], 1, 1));
        // A long text equals no short one, even its own prefix.
        assert_ne!(Text::of(&long), Text::of(&long[..LONG_PREFIX]));
        assert_eq!(Text::kept(&long[..LONG_PREFIX], true), Text::of(&long));
        assert_eq!(Text::kept(&long, true), Text::of(&long));
        assert_eq!(
            Text::of(&"é".repeat(SCOPE_MAX_BYTES / 2)),
            Text::Full("é".repeat(SCOPE_MAX_BYTES / 2))
        );
    }

    #[test]
    fn rows_and_paths_display_as_scope_texts() {
        let rows = [
            Row::new(1, "a", "", 1, 9, None),
            Row::new(2, "S<T>", "From<u8>", 2, 8, Some(0)),
            Row::new(3, "f", "", 3, 4, Some(1)),
        ];
        let paths = name_paths(&rows);
        assert_eq!(path_text(&paths[2]), "rust:mod a/impl S<T>[From<u8>]/fn f");
        assert_eq!(rows[1].to_string(), "impl S<T>[From<u8>] 2–8 in #0");
        assert_eq!(rows[0].to_string(), "mod a 1–9");
    }

    #[test]
    fn restricted_crates_are_reported_by_counts_only() {
        let rec = record(vec![
            item(1, "a", "", (1, 3), None, true),
            item(3, "secret", "", (2, 2), Some(0), true),
        ]);
        let mut r = Report::default();
        r.add(compare(
            "crates/moirai-model/src/x.rs",
            &scan_side(SRC),
            &rec,
        ));
        r.add(compare(
            "crates/moirai-replay/src/x.rs",
            &scan_side(SRC),
            &rec,
        ));
        let text = r.to_string();
        assert!(
            text.contains(
                "crates/moirai-model/src/x.rs: 1 missing, 2 extra (items not named: docs/m0/PLAN.md §3.1)"
            ),
            "{text}"
        );
        // The same disagreement in a crate every role may read is listed item by item.
        assert!(
            text.contains("crates/moirai-replay/src/x.rs:\n  missing ×1: rust:mod a/fn secret 2–2"),
            "{text}"
        );
        assert_eq!(text.matches("secret").count(), 1, "{text}");
        // Asked for, the names of every file are given.
        let named = r.named().to_string();
        assert!(
            named.contains("crates/moirai-model/src/x.rs:\n  missing ×1: rust:mod a/fn secret 2–2"),
            "{named}"
        );
        assert_eq!(named.matches("secret").count(), 2, "{named}");
        assert!(!named.contains("items not named"), "{named}");
        // The summary is the report's head: counts and rates, no file named.
        let summary = r.summary().to_string();
        assert!(text.starts_with(&summary), "{summary}\n{text}");
        assert_eq!(summary.lines().count(), 2 + Level::ALL.len(), "{summary}");
        assert!(!summary.contains("crates/"), "{summary}");
    }

    /// The quoted patterns of every `deny_read` array of `xtask/roles.toml`, written on one line or over several (its
    /// patterns hold no `"` and no escape).
    fn deny_read_patterns(roles: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut inside = false;
        for line in roles.lines() {
            let line = line.trim();
            let rest = if let Some(r) = line.strip_prefix("deny_read = [") {
                inside = true;
                r
            } else if inside {
                line
            } else {
                continue;
            };
            let mut quoted: Option<String> = None;
            for c in rest.chars() {
                if let Some(q) = quoted.as_mut() {
                    if c == '"' {
                        out.extend(quoted.take());
                    } else {
                        q.push(c);
                    }
                } else if c == '"' {
                    quoted = Some(String::new());
                } else if c == ']' {
                    inside = false;
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn counts_only_covers_every_path_a_role_must_not_read() {
        let roles_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../xtask/roles.toml");
        let roles = std::fs::read_to_string(roles_path).expect("xtask/roles.toml");
        let patterns = deny_read_patterns(&roles);
        assert!(
            patterns.len() >= 8,
            "the deny_read lists were read: {patterns:?}"
        );
        for p in &patterns {
            // Every pattern lies in one crate (`crates/<name>/…`); its crate is counted only.
            let mut parts = p.splitn(3, '/');
            let (Some("crates"), Some(name)) = (parts.next(), parts.next()) else {
                panic!("a deny_read pattern outside crates/: {p}")
            };
            let probe = format!("crates/{name}/src/lib.rs");
            assert!(counts_only(&probe), "{p} ({probe})");
        }
        for path in [
            "crates/moirai-model/src/lib.rs",
            "crates/moirai-toylog/src/bug.rs",
            "crates/moirai-vfs-sim/src/lib.rs",
            "crates/moirai-files/src/scan.rs",
        ] {
            assert!(counts_only(path), "{path}");
        }
        for path in [
            "crates/moirai-models/x.rs",
            "crates/moirai-replay/src/lib.rs",
            "crates/moirai-tsoracle/src/scan.rs",
            "crates/moirai-lqbench/src/lib.rs",
            "xtask/src/main.rs",
        ] {
            assert!(!counts_only(path), "{path}");
        }
    }

    #[test]
    fn a_file_with_errors_may_have_extra_items_only() {
        // The scanner reports `S` too, which the oracle (with an error) dropped: the claim still holds.
        let mut rec = record(vec![
            item(1, "a", "", (1, 3), None, true),
            item(3, "f", "", (2, 2), Some(0), true),
        ]);
        rec.errors = 1;
        let d = compare("x.rs", &scan_side(SRC), &rec);
        assert!(d.claimed_agree() && !d.agrees(), "{d:?}");
        rec.items[1].start = 1;
        let d = compare("x.rs", &scan_side(SRC), &rec);
        assert!(!d.claimed_agree(), "{d:?}");
    }

    #[test]
    fn an_empty_report_is_complete() {
        let r = Report::default();
        assert_eq!(r.rate(Level::Header), 100.0);
        assert!(r.to_string().starts_with("files: 0 compared"));
    }
}
