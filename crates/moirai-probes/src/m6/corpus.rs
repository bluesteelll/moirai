//! Measurement 6's corpora (cut D-1): the owner's notes through `manifest.tsv`, the plan sections of the repository's
//! `docs/**`, where a run finds them, the split into training and measured inputs, and the synthetic corpora of the
//! tests and of a smoke that lacks the real ones.
//!
//! - **Notes.** `manifest.tsv` has `#` comment lines, the header `body\tbytes\tsha256\tsource` and one row of 4 fields
//!   per body. `body` is a relative `/` path below the manifest's directory, `bytes` is decimal, equals the file's
//!   length and is at most [`MAX_INPUT`]. `sha256` is not checked (cut N-3). Bodies keep the manifest's order.
//! - **Plan sections.** Every `*.md` file below `docs/`, in bytewise order of its relative path, split at ATX headings
//!   outside fenced code blocks ([`sections`]).
//! - **Where they are.** A recording run takes them from `--out` = `<P>/measurements/6` ([`from_out`]); a smoke from
//!   the checkout the binary was built in ([`from_checkout`], [`smoke_notes`], [`smoke_plan`]). For one tree both name
//!   the same manifest and the same `docs` directory.
//! - **The split.** An input with id `i` trains the dictionary when `xxh3_64(i) & 1 == 0` and is measured otherwise
//!   ([`split`]): deterministic, disjoint and independent of order.
//!
//! The synthetic corpora hold a seeded word salad over a fixed list of common words, never owner text.

use crate::tail::BODY_RAW_CAP;
use std::path::{Component, Path, PathBuf};

/// The largest input: a body's raw bytes are at most 64 KiB ([`crate::tail::BODY_RAW_CAP`]), and a plan section is
/// cut into pieces of at most this length.
pub const MAX_INPUT: usize = BODY_RAW_CAP as usize;

/// The manifest's header line.
pub const HEADER: &str = "body\tbytes\tsha256\tsource";

/// One input of a corpus: its id (the manifest's `body`, or the section id) and its bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Input {
    /// The id; the split is keyed by it.
    pub id: String,
    /// The raw bytes.
    pub bytes: Vec<u8>,
}

/// One row of `manifest.tsv`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    /// The body's relative `/` path below the manifest's directory.
    pub body: String,
    /// Its length in bytes.
    pub bytes: usize,
}

/// A relative `/` path with no empty, `.` or `..` component, no `\` and no `:`.
fn relative_path(p: &str) -> bool {
    !p.is_empty()
        && !p.contains(['\\', ':'])
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Parses `manifest.tsv`: `#` lines are skipped, the first other line is [`HEADER`], and each row has 4 fields, a
/// relative `body` path and a decimal `bytes` of at most [`MAX_INPUT`].
pub fn parse_manifest(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.starts_with('#'));
    match lines.next() {
        Some((_, h)) if h == HEADER => {}
        Some((i, _)) => {
            return Err(format!(
                "line {}: the header is not {}",
                i + 1,
                HEADER.escape_default()
            ));
        }
        None => return Err("no header".into()),
    }
    let mut rows = Vec::new();
    for (i, l) in lines {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 4 {
            return Err(format!("line {}: {} fields, 4 expected", i + 1, f.len()));
        }
        if !relative_path(f[0]) {
            return Err(format!(
                "line {}: body '{}' is not a relative / path",
                i + 1,
                f[0]
            ));
        }
        let bytes = if !f[1].is_empty() && f[1].bytes().all(|b| b.is_ascii_digit()) {
            f[1].parse::<usize>().ok()
        } else {
            None
        };
        match bytes {
            Some(b) if b <= MAX_INPUT => rows.push(Row {
                body: f[0].to_string(),
                bytes: b,
            }),
            Some(b) => {
                return Err(format!(
                    "line {}: {b} bytes, above the {MAX_INPUT}-byte cap",
                    i + 1
                ));
            }
            None => {
                return Err(format!(
                    "line {}: bytes '{}' is not a decimal count",
                    i + 1,
                    f[1]
                ));
            }
        }
    }
    Ok(rows)
}

/// Loads the bodies `manifest` lists, in its order, each checked against its `bytes`.
pub fn load_notes(manifest: &Path) -> Result<Vec<Input>, String> {
    let text =
        std::fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let rows = parse_manifest(&text).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let dir = manifest.parent().unwrap_or(Path::new(""));
    rows.into_iter()
        .map(|r| {
            let p = r.body.split('/').fold(dir.to_path_buf(), |p, c| p.join(c));
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            if bytes.len() != r.bytes {
                return Err(format!(
                    "{}: {} bytes, the manifest says {}",
                    p.display(),
                    bytes.len(),
                    r.bytes
                ));
            }
            Ok(Input { id: r.body, bytes })
        })
        .collect()
}

/// A fence's opening: up to 3 spaces, then at least 3 backticks or tildes; its character and length.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Whether `line` closes a fence of `n` characters `c`: up to 3 spaces, at least `n` of `c`, then only whitespace.
fn fence_close(line: &str, c: char, n: usize) -> bool {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return false;
    }
    let run = t.chars().take_while(|x| *x == c).count();
    run >= n && t[run * c.len_utf8()..].trim().is_empty()
}

/// An ATX heading: 1 to 6 `#` at the line start, then a space, a tab or the line end.
fn heading(line: &str) -> bool {
    let n = line.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&n) && matches!(line.as_bytes().get(n), None | Some(b' ' | b'\t'))
}

/// The largest char boundary of `s` at or below `i`.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Cuts `s` into pieces of at most [`MAX_INPUT`] bytes at line ends, or at a char boundary inside a longer line.
fn pieces(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut end) = (0, 0);
    for line in s.split_inclusive('\n') {
        let le = end + line.len();
        if le - start > MAX_INPUT {
            if end > start {
                out.push(&s[start..end]);
                start = end;
            }
            while le - start > MAX_INPUT {
                let cut = floor_boundary(s, start + MAX_INPUT);
                out.push(&s[start..cut]);
                start = cut;
            }
        }
        end = le;
    }
    if end > start {
        out.push(&s[start..end]);
    }
    out
}

/// The sections of the file `docs/<rel>`: split at ATX headings outside fenced code blocks (``` or ~~~). Section `k`
/// runs from its heading to the line before the next one; the text before the first heading is section 0. A section
/// above [`MAX_INPUT`] bytes is cut by [`pieces`], and piece `p` adds `.<p>` to its id `docs/<rel>#<k>`. Sections and
/// pieces that are only whitespace are dropped.
pub fn sections(rel: &str, text: &str) -> Vec<Input> {
    let mut secs = vec![String::new()];
    let mut fence: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        match fence {
            Some((c, n)) => {
                if fence_close(bare, c, n) {
                    fence = None;
                }
            }
            None => {
                if let Some(f) = fence_open(bare) {
                    fence = Some(f);
                } else if heading(bare) {
                    secs.push(String::new());
                }
            }
        }
        secs.last_mut().expect("one section").push_str(line);
    }
    let mut out = Vec::new();
    for (k, s) in secs.iter().enumerate() {
        let ps = pieces(s);
        let several = ps.len() > 1;
        for (p, piece) in ps.into_iter().enumerate() {
            if piece.trim().is_empty() {
                continue;
            }
            let id = if several {
                format!("docs/{rel}#{k}.{p}")
            } else {
                format!("docs/{rel}#{k}")
            };
            out.push(Input {
                id,
                bytes: piece.as_bytes().to_vec(),
            });
        }
    }
    out
}

/// The `*.md` files below `dir`, as `/` paths relative to `root`.
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in rd {
        let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
        let p = e.path();
        let ft = e.file_type().map_err(|e| format!("{}: {e}", p.display()))?;
        if ft.is_dir() {
            walk(root, &p, out)?;
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "md") {
            let rel = p.strip_prefix(root).map_err(|e| e.to_string())?;
            let parts: Option<Vec<&str>> =
                rel.components().map(|c| c.as_os_str().to_str()).collect();
            out.push(
                parts
                    .ok_or_else(|| format!("{}: a path that is not valid Unicode", p.display()))?
                    .join("/"),
            );
        }
    }
    Ok(())
}

/// The plan sections of every `*.md` file below `docs`, files in bytewise order of their relative path.
pub fn load_plan(docs: &Path) -> Result<Vec<Input>, String> {
    let mut files = Vec::new();
    walk(docs, docs, &mut files)?;
    files.sort();
    let mut out = Vec::new();
    for rel in files {
        let p = rel.split('/').fold(docs.to_path_buf(), |p, c| p.join(c));
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        out.extend(sections(&rel, &text));
    }
    Ok(out)
}

/// The training and the measured inputs, each in id order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Split {
    /// The inputs the dictionary is trained on.
    pub training: Vec<Input>,
    /// The inputs that are measured.
    pub measured: Vec<Input>,
}

/// Whether the input `id` trains the dictionary: `xxh3_64(id) & 1 == 0`.
pub fn in_training(id: &str) -> bool {
    xxhash_rust::xxh3::xxh3_64(id.as_bytes()) & 1 == 0
}

/// Splits `inputs` by [`in_training`], each side in id order; duplicate ids or an empty side are an error.
pub fn split(mut inputs: Vec<Input>) -> Result<Split, String> {
    inputs.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(w) = inputs.windows(2).find(|w| w[0].id == w[1].id) {
        return Err(format!("the id '{}' appears twice", w[0].id));
    }
    let (training, measured): (Vec<Input>, Vec<Input>) =
        inputs.into_iter().partition(|i| in_training(&i.id));
    if training.is_empty() || measured.is_empty() {
        return Err(format!(
            "an empty side: {} training and {} measured inputs",
            training.len(),
            measured.len()
        ));
    }
    Ok(Split { training, measured })
}

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }

    /// Log-uniform in `[lo, hi]`.
    fn log_uniform(&mut self, lo: u64, hi: u64) -> u64 {
        let u = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        let v = (lo as f64) * ((hi as f64) / (lo as f64)).powf(u);
        (v.round() as u64).clamp(lo, hi)
    }
}

/// The word list of the synthetic corpora: 200 common English and engineering words, separated by spaces.
const WORDS: &str = "the of and to a in is it that for on with as this be are by not or from at we an but if when each \
     one two all can should must will then so no only first next last new old same other every before \
     after into out up down more less file node edge task rule note test build run gate commit branch \
     merge tree path link query index page frame block record log tail head base store read write open \
     close map list set key value field column row table graph memory cache disk time size byte count \
     limit budget speed cost plan step stage unit role agent session tool hook user owner review report \
     spec design check error fault crash lock version change fix issue idea reason result answer question \
     decision option default config flag name id hash codec dictionary sample split train measure \
     budgeted window night morning day week small large fast slow safe simple clear exact stable random \
     fixed shut closed ready done keep drop move copy load save send find show make take give use need \
     want like know see say work start stop wait return call line text word draft batch";

/// A Zipf-weighted word: rank `r` has weight 1 / (r + 1).
fn word(rng: &mut Rng, cumulative: &[(u64, &'static str)]) -> &'static str {
    let total = cumulative.last().expect("words").0;
    let u = rng.next() % total;
    cumulative[cumulative.partition_point(|c| c.0 <= u)].1
}

/// The words with their cumulative Zipf weights.
fn cumulative() -> Vec<(u64, &'static str)> {
    let mut c = 0;
    WORDS
        .split_ascii_whitespace()
        .zip(1u64..)
        .map(|(w, r)| {
            c += 1_000_000 / r;
            (c, w)
        })
        .collect()
}

/// Between `lo` and `hi` words, joined by spaces.
fn words(rng: &mut Rng, cum: &[(u64, &'static str)], lo: u64, hi: u64) -> String {
    let n = rng.range(lo, hi);
    (0..n).map(|_| word(rng, cum)).collect::<Vec<_>>().join(" ")
}

/// A markdown text of exactly `len` ASCII bytes: a title, then paragraphs, `##` headings and bullet lists.
fn salad(rng: &mut Rng, cum: &[(u64, &'static str)], len: usize) -> String {
    let mut s = format!("# {}\n\n", words(rng, cum, 2, 6));
    while s.len() < len {
        match rng.range(0, 9) {
            0 => s.push_str(&format!("## {}\n\n", words(rng, cum, 2, 5))),
            1 | 2 => {
                for _ in 0..rng.range(2, 5) {
                    s.push_str(&format!("- {}\n", words(rng, cum, 3, 10)));
                }
                s.push('\n');
            }
            _ => {
                for _ in 0..rng.range(1, 4) {
                    let mut w = words(rng, cum, 5, 15);
                    w[..1].make_ascii_uppercase();
                    s.push_str(&w);
                    s.push_str(". ");
                }
                s.push_str("\n\n");
            }
        }
    }
    s.truncate(len);
    s
}

/// The synthetic notes: 40 bodies of log-uniform lengths in [200, 16,384] and one of exactly [`MAX_INPUT`] bytes,
/// with ids `synthetic/note-<i>.md`; deterministic per seed.
pub fn synthetic_notes(seed: u64) -> Vec<Input> {
    let mut rng = Rng(seed);
    let cum = cumulative();
    (0..41)
        .map(|i| {
            let len = if i == 17 {
                MAX_INPUT
            } else {
                rng.log_uniform(200, 16_384) as usize
            };
            Input {
                id: format!("synthetic/note-{i:03}.md"),
                bytes: salad(&mut rng, &cum, len).into_bytes(),
            }
        })
        .collect()
}

/// The synthetic `docs` set: 8 files as (relative path, text), two of them in a subdirectory, with fenced blocks
/// (``` and ~~~) that hold `#` lines; deterministic per seed.
pub fn synthetic_docs(seed: u64) -> Vec<(String, String)> {
    let mut rng = Rng(seed ^ 0x6D6F_6972_6169_0006);
    let cum = cumulative();
    (0..8)
        .map(|f| {
            let rel = if f < 6 {
                format!("plan-{f}.md")
            } else {
                format!("sub/part-{f}.md")
            };
            let mut t = String::new();
            for k in 0..rng.range(3, 7) {
                let level = "#".repeat(1 + (k as usize % 3));
                t.push_str(&format!("{level} {}\n\n", words(&mut rng, &cum, 3, 3)));
                let n = rng.log_uniform(300, 4_000) as usize;
                t.push_str(&salad(&mut rng, &cum, n).replace("# ", "Title: "));
                t.push('\n');
                if rng.range(0, 2) == 0 {
                    let fence = if k % 2 == 0 { "```" } else { "~~~" };
                    t.push_str(&format!(
                        "{fence}sh\n# {}\nrun --flag\n## {}\n{fence}\n\n",
                        words(&mut rng, &cum, 4, 4),
                        words(&mut rng, &cum, 2, 2)
                    ));
                }
            }
            (rel, t)
        })
        .collect()
}

/// The sections of [`synthetic_docs`].
pub fn synthetic_plan(seed: u64) -> Vec<Input> {
    synthetic_docs(seed)
        .iter()
        .flat_map(|(rel, t)| sections(rel, t))
        .collect()
}

/// Where a run's two corpora are: the notes' manifest and the `docs` directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Places {
    /// `<P>/codec-bodies/manifest.tsv`.
    pub manifest: PathBuf,
    /// `<P>/../docs`.
    pub docs: PathBuf,
}

/// The corpora of a recording run: `out` must be `<P>/measurements/6` ([MP §9.7]; the runner's `--out`), and then
/// `P` is the private root, the notes are `P/codec-bodies/manifest.tsv` and the plan is `P`'s parent's `docs`.
pub fn from_out(out: &Path) -> Result<Places, String> {
    let mut c = out.components();
    let six = c.next_back();
    let measurements = c.next_back();
    let private: PathBuf = c.collect();
    let shape = matches!(
        (measurements, six),
        (Some(Component::Normal(m)), Some(Component::Normal(s))) if m == "measurements" && s == "6"
    );
    match private.parent() {
        Some(repo) if shape && private.file_name().is_some() => Ok(Places {
            manifest: private.join("codec-bodies").join("manifest.tsv"),
            docs: repo.join("docs"),
        }),
        _ => Err(format!(
            "--out {} is not <private>/measurements/6, so the corpora cannot be found",
            out.display()
        )),
    }
}

/// The corpora of a smoke: those of the checkout the binary was built in, `<checkout>/private/codec-bodies/
/// manifest.tsv` and `<checkout>/docs`.
pub fn from_checkout(checkout: &Path) -> Places {
    Places {
        manifest: checkout
            .join("private")
            .join("codec-bodies")
            .join("manifest.tsv"),
        docs: checkout.join("docs"),
    }
}

/// Where a smoke's corpus came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// Read from this manifest or `docs` directory.
    Read(PathBuf),
    /// Synthetic, because this path is absent, or because no checkout was given.
    Synthetic(Option<PathBuf>),
}

/// A smoke's corpus and where it came from.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// The inputs.
    pub inputs: Vec<Input>,
    /// Their origin.
    pub origin: Origin,
}

impl Loaded {
    /// The smoke's line for corpus `name`: `m6 smoke corpus <name> <path> <k> inputs <b> bytes`, or `m6 smoke corpus
    /// <name> synthetic (<path> absent) <k> inputs <b> bytes`. Paths and counts only, never body text.
    pub fn line(&self, name: &str) -> String {
        let k = self.inputs.len();
        let b: usize = self.inputs.iter().map(|i| i.bytes.len()).sum();
        match &self.origin {
            Origin::Read(p) => format!(
                "m6 smoke corpus {name} {} {k} inputs {b} bytes",
                p.display()
            ),
            Origin::Synthetic(Some(p)) => format!(
                "m6 smoke corpus {name} synthetic ({} absent) {k} inputs {b} bytes",
                p.display()
            ),
            Origin::Synthetic(None) => {
                format!("m6 smoke corpus {name} synthetic (no checkout) {k} inputs {b} bytes")
            }
        }
    }
}

/// The smoke's notes: the checkout's manifest when it is a file (loaded and checked as a recording run's), else
/// [`synthetic_notes`].
pub fn smoke_notes(checkout: Option<&Path>, seed: u64) -> Result<Loaded, String> {
    let m = checkout.map(|c| from_checkout(c).manifest);
    match m {
        Some(m) if m.is_file() => Ok(Loaded {
            inputs: load_notes(&m)?,
            origin: Origin::Read(m),
        }),
        m => Ok(Loaded {
            inputs: synthetic_notes(seed),
            origin: Origin::Synthetic(m),
        }),
    }
}

/// The smoke's plan sections: the checkout's `docs` when it is a directory, else [`synthetic_plan`].
pub fn smoke_plan(checkout: Option<&Path>, seed: u64) -> Result<Loaded, String> {
    let d = checkout.map(|c| from_checkout(c).docs);
    match d {
        Some(d) if d.is_dir() => Ok(Loaded {
            inputs: load_plan(&d)?,
            origin: Origin::Read(d),
        }),
        d => Ok(Loaded {
            inputs: synthetic_plan(seed),
            origin: Origin::Synthetic(d),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::scratch_dir;
    use std::collections::BTreeSet;

    fn ids(v: &[Input]) -> Vec<&str> {
        v.iter().map(|i| i.id.as_str()).collect()
    }

    /// Writes `files` (relative `/` paths) below `root`.
    fn tree(root: &Path, files: &[(&str, &[u8])]) {
        for (rel, bytes) in files {
            let p = rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, bytes).unwrap();
        }
    }

    /// The manifest parser skips `#` lines, needs the header, and refuses rows of other than 4 fields, a body path
    /// that is absolute or climbs, a `bytes` that is not decimal, and one above 64 KiB.
    #[test]
    fn manifest_rows_parse_and_refuse_malformed_lines() {
        let good = "# made by the orchestrator\nbody\tbytes\tsha256\tsource\nnotes/a.md\t12\tab\tx\n# mid\nnotes/b/c.md\t65536\tcd\ty\n";
        assert_eq!(
            parse_manifest(good).unwrap(),
            [
                Row {
                    body: "notes/a.md".into(),
                    bytes: 12
                },
                Row {
                    body: "notes/b/c.md".into(),
                    bytes: 65_536
                }
            ]
        );
        assert_eq!(parse_manifest("body\tbytes\tsha256\tsource\n").unwrap(), []);
        let h = "body\tbytes\tsha256\tsource\n";
        for (text, why) in [
            ("".to_string(), "no header"),
            ("# only\n".into(), "no header"),
            ("path\tbytes\tsha256\tsource\n".into(), "line 1: the header"),
            (format!("{h}notes/a.md\t12\tab\n"), "line 2: 3 fields"),
            (format!("{h}notes/a.md\t12\tab\tx\ty\n"), "line 2: 5 fields"),
            (format!("{h}\n"), "line 2: 1 fields"),
            (format!("{h}/etc/a.md\t12\tab\tx\n"), "not a relative"),
            (format!("{h}notes/../a.md\t12\tab\tx\n"), "not a relative"),
            (format!("{h}C:/a.md\t12\tab\tx\n"), "not a relative"),
            (format!("{h}notes\\a.md\t12\tab\tx\n"), "not a relative"),
            (format!("{h}notes//a.md\t12\tab\tx\n"), "not a relative"),
            (format!("{h}notes/a.md\t-1\tab\tx\n"), "not a decimal"),
            (format!("{h}notes/a.md\t0x10\tab\tx\n"), "not a decimal"),
            (format!("{h}notes/a.md\t\tab\tx\n"), "not a decimal"),
            (
                format!("{h}notes/a.md\t65537\tab\tx\n"),
                "above the 65536-byte cap",
            ),
        ] {
            let e = parse_manifest(&text).unwrap_err();
            assert!(e.contains(why), "{text:?}: {e}");
        }
    }

    /// The notes load from a synthetic tree in the manifest's order, with bodies below the manifest's directory; a
    /// row whose `bytes` differs from its file's length, or a missing body, fails naming the path.
    #[test]
    fn notes_load_from_a_synthetic_tree_and_check_lengths() {
        let root = scratch_dir("m6-notes");
        let m = root.join("codec-bodies").join("manifest.tsv");
        tree(
            &root,
            &[
                ("codec-bodies/notes/z.md", b"zeta body"),
                ("codec-bodies/notes/a/b.md", b"alpha"),
                (
                    "codec-bodies/manifest.tsv",
                    b"# c\nbody\tbytes\tsha256\tsource\nnotes/z.md\t9\tff\ts\nnotes/a/b.md\t5\tee\tt\n",
                ),
            ],
        );
        let v = load_notes(&m).unwrap();
        assert_eq!(
            ids(&v),
            ["notes/z.md", "notes/a/b.md"],
            "the manifest's order"
        );
        assert_eq!(v[0].bytes, b"zeta body");
        assert_eq!(v[1].bytes, b"alpha");
        std::fs::write(
            &m,
            "body\tbytes\tsha256\tsource\nnotes/z.md\t9\tff\ts\nnotes/a/b.md\t6\tee\tt\n",
        )
        .unwrap();
        let e = load_notes(&m).unwrap_err();
        assert!(
            e.contains("b.md") && e.contains("5 bytes, the manifest says 6"),
            "{e}"
        );
        std::fs::write(&m, "body\tbytes\tsha256\tsource\nnotes/y.md\t1\tff\ts\n").unwrap();
        let e = load_notes(&m).unwrap_err();
        assert!(e.contains("y.md"), "{e}");
        let e = load_notes(&root.join("absent.tsv")).unwrap_err();
        assert!(e.contains("absent.tsv"), "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sections split at ATX headings (1-6 `#`, then a space or the line end), never inside ``` or ~~~ fences, and
    /// the text before the first heading is section 0; `#tag` and 7 `#` are not headings; whitespace sections drop.
    #[test]
    fn sections_split_at_headings_outside_fences() {
        let text = "intro line\n# One\nbody one\n```sh\n# comment in a fence\n~~~\n## still fenced\n```\n#tag is text\n####### seven\n## Two\n~~~~\n# fenced\n```\n~~~~\ntext two\n";
        let s = sections("dir/f.md", text);
        assert_eq!(
            ids(&s),
            ["docs/dir/f.md#0", "docs/dir/f.md#1", "docs/dir/f.md#2"],
            "{s:?}"
        );
        assert_eq!(s[0].bytes, b"intro line\n");
        assert_eq!(
            String::from_utf8(s[1].bytes.clone()).unwrap(),
            "# One\nbody one\n```sh\n# comment in a fence\n~~~\n## still fenced\n```\n#tag is text\n####### seven\n"
        );
        assert_eq!(
            String::from_utf8(s[2].bytes.clone()).unwrap(),
            "## Two\n~~~~\n# fenced\n```\n~~~~\ntext two\n"
        );
        let joined: Vec<u8> = s.iter().flat_map(|i| i.bytes.clone()).collect();
        assert_eq!(joined, text.as_bytes(), "nothing lost");
        // A file that starts with a heading has no section 0, and a whitespace-only section 0 is dropped; a bare `#`
        // line is a heading.
        assert_eq!(
            ids(&sections("g.md", "# A\nx\n# B\ny")),
            ["docs/g.md#1", "docs/g.md#2"]
        );
        assert_eq!(
            ids(&sections("w.md", "  \n\n# A\nx\n#\n")),
            ["docs/w.md#1", "docs/w.md#2"]
        );
        // An unclosed fence runs to the end.
        assert_eq!(ids(&sections("h.md", "# A\n```\n# B\n")), ["docs/h.md#1"]);
    }

    /// A section above 64 KiB is cut into pieces of at most 65,536 bytes at line ends, or at a UTF-8 boundary
    /// inside a longer line; the pieces add `.<p>` and together are the section.
    #[test]
    fn long_sections_split_below_64_kib() {
        let line = format!("{}\n", "x".repeat(999));
        let mut text = String::from("# Big\n");
        for _ in 0..100 {
            text.push_str(&line);
        }
        let s = sections("big.md", &text);
        assert_eq!(ids(&s), ["docs/big.md#1.0", "docs/big.md#1.1"]);
        assert_eq!(
            s[0].bytes.len(),
            6 + 65 * 1_000,
            "line ends: 65 whole lines after the heading"
        );
        assert!(s.iter().all(|i| i.bytes.len() <= MAX_INPUT));
        assert_eq!(s.iter().map(|i| i.bytes.len()).sum::<usize>(), text.len());
        // One 150,003-byte line of 3-byte chars after a 2-byte ASCII prefix: cut at the last char boundary at or
        // below each 65,536-byte mark, so 65,534 + 65,535 + 18,934 bytes.
        let long = format!("ab{}\n", "\u{20AC}".repeat(50_000));
        let s = sections("long.md", &long);
        assert_eq!(
            ids(&s),
            ["docs/long.md#0.0", "docs/long.md#0.1", "docs/long.md#0.2"]
        );
        let lens: Vec<usize> = s.iter().map(|i| i.bytes.len()).collect();
        assert_eq!(lens, [65_534, 65_535, 18_934]);
        assert!(s.iter().all(|i| std::str::from_utf8(&i.bytes).is_ok()));
        let joined: Vec<u8> = s.iter().flat_map(|i| i.bytes.clone()).collect();
        assert_eq!(joined, long.as_bytes());
        // Exactly 65,536 bytes stays whole.
        let exact = format!("# E\n{}\n", "y".repeat(MAX_INPUT - 5));
        assert_eq!(ids(&sections("e.md", &exact)), ["docs/e.md#1"]);
    }

    /// The walk reads every `*.md` file below `docs/`, recursively, in bytewise order of the relative path; other
    /// files are skipped.
    #[test]
    fn docs_walk_reads_markdown_in_path_order() {
        let root = scratch_dir("m6-docs");
        let docs = root.join("docs");
        tree(
            &docs,
            &[
                ("b.md", b"# B\nb\n"),
                ("a/z.md", b"# Z\nz\n"),
                ("a/Y.md", b"# Y\ny\n"),
                ("a-b.md", b"pre\n# AB\nab\n"),
                ("notes.txt", b"# not markdown\n"),
                ("c/d/e.md", b"# E\ne\n"),
            ],
        );
        let v = load_plan(&docs).unwrap();
        assert_eq!(
            ids(&v),
            [
                "docs/a-b.md#0",
                "docs/a-b.md#1",
                "docs/a/Y.md#1",
                "docs/a/z.md#1",
                "docs/b.md#1",
                "docs/c/d/e.md#1"
            ]
        );
        assert_eq!(v[2].bytes, b"# Y\ny\n");
        assert!(
            load_plan(&root.join("nodocs"))
                .unwrap_err()
                .contains("nodocs")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The split is keyed by id (`xxh3_64(id) & 1 == 0` trains): disjoint, covering, the same for any input order,
    /// each side in id order; duplicate ids and an empty side are errors.
    #[test]
    fn split_is_disjoint_deterministic_and_keyed_by_id() {
        let inputs: Vec<Input> = (0..64)
            .map(|i| Input {
                id: format!("n/{i}.md"),
                bytes: vec![i as u8; 3],
            })
            .collect();
        let s = split(inputs.clone()).unwrap();
        let mut rev = inputs.clone();
        rev.reverse();
        assert_eq!(split(rev).unwrap(), s, "independent of order");
        assert!(s.training.iter().all(|i| in_training(&i.id)));
        assert!(s.measured.iter().all(|i| !in_training(&i.id)));
        let t: BTreeSet<&str> = ids(&s.training).into_iter().collect();
        let m: BTreeSet<&str> = ids(&s.measured).into_iter().collect();
        assert!(t.is_disjoint(&m));
        assert_eq!(t.len() + m.len(), 64);
        assert!(
            t.len() >= 16 && m.len() >= 16,
            "about half each: {} {}",
            t.len(),
            m.len()
        );
        assert!(s.training.windows(2).all(|w| w[0].id < w[1].id));
        assert!(s.measured.windows(2).all(|w| w[0].id < w[1].id));
        // Keyed by id: the same id lands on the same side in another corpus.
        let one = s.measured[0].clone();
        let other = split(vec![
            one.clone(),
            s.training[0].clone(),
            Input {
                id: "x".into(),
                bytes: vec![],
            },
        ])
        .unwrap();
        assert!(other.measured.contains(&one));
        let only_t: Vec<Input> = s.training.clone();
        assert!(split(only_t).unwrap_err().contains("an empty side"));
        let mut dup = inputs;
        dup.push(dup[3].clone());
        assert!(split(dup).unwrap_err().contains("twice"));
    }

    /// The synthetic corpora are deterministic per seed, ASCII, within [200, 65,536] bytes with one body of exactly
    /// 65,536, and the synthetic docs hold fenced `#` lines that do not split their sections.
    #[test]
    fn synthetic_corpora_are_deterministic_and_bounded() {
        let a = synthetic_notes(7);
        assert_eq!(a, synthetic_notes(7));
        assert_ne!(a, synthetic_notes(8));
        assert_eq!(a.len(), 41);
        assert!(a.iter().all(|i| (200..=MAX_INPUT).contains(&i.bytes.len())));
        assert_eq!(a.iter().filter(|i| i.bytes.len() == MAX_INPUT).count(), 1);
        assert!(a.iter().all(|i| i.bytes.is_ascii()));
        assert!(a.iter().any(|i| i.bytes.starts_with(b"# ")));
        let lens: BTreeSet<usize> = a.iter().map(|i| i.bytes.len()).collect();
        assert!(lens.len() > 30, "lengths vary");
        let d = synthetic_docs(7);
        assert_eq!(d, synthetic_docs(7));
        assert!(d.iter().any(|(_, t)| t.contains("```sh\n# ")));
        assert!(d.iter().any(|(_, t)| t.contains("~~~sh\n# ")));
        let p = synthetic_plan(7);
        assert_eq!(p, synthetic_plan(7));
        // A fenced `# ...` line never starts a section: its next line `run --flag` would then be a section's second.
        assert!(
            p.iter()
                .all(|i| String::from_utf8_lossy(&i.bytes).lines().nth(1) != Some("run --flag"))
        );
        assert!(p.iter().all(|i| i.bytes.len() <= MAX_INPUT));
        let s = split(p).unwrap();
        assert!(!s.training.is_empty() && !s.measured.is_empty());
        split(a).unwrap();
    }

    /// `--out` = `<P>/measurements/6` (with or without a trailing separator) names `P/codec-bodies/manifest.tsv`
    /// and `P`'s parent's `docs`; any other shape is refused naming the path.
    #[test]
    fn corpora_are_found_from_out() {
        let repo = Path::new("/r").join("main");
        let out = repo.join("private").join("measurements").join("6");
        let want = Places {
            manifest: repo
                .join("private")
                .join("codec-bodies")
                .join("manifest.tsv"),
            docs: repo.join("docs"),
        };
        assert_eq!(from_out(&out).unwrap(), want);
        let trailing = PathBuf::from(format!("{}/", out.display()));
        assert_eq!(from_out(&trailing).unwrap(), want);
        for bad in [
            repo.join("private").join("measurements").join("10"),
            repo.join("private").join("m").join("6"),
            repo.join("private").join("measurements"),
            PathBuf::from("measurements/6"),
            PathBuf::from("/measurements/6"),
            PathBuf::new(),
        ] {
            let e = from_out(&bad).unwrap_err();
            assert!(e.contains(&bad.display().to_string()), "{bad:?}: {e}");
        }
    }

    /// The smoke reads the build checkout's corpora when they are present (a synthetic tree), says where from, and
    /// uses the synthetic ones when they are absent (an empty tree) or no checkout is given; for one tree the
    /// checkout and `--out` locators agree.
    #[test]
    fn corpora_are_found_from_the_build_checkout_or_are_synthetic() {
        let t = scratch_dir("m6-checkout");
        tree(
            &t,
            &[
                ("private/codec-bodies/notes/one.md", b"first body"),
                ("private/codec-bodies/notes/two.md", b"second"),
                (
                    "private/codec-bodies/manifest.tsv",
                    b"body\tbytes\tsha256\tsource\nnotes/one.md\t10\t0\ts\nnotes/two.md\t6\t0\ts\n",
                ),
                ("docs/x.md", b"# X\nx text\n## Y\ny text\n"),
            ],
        );
        let out = t.join("private").join("measurements").join("6");
        assert_eq!(
            from_out(&out).unwrap(),
            from_checkout(&t),
            "the locators agree"
        );
        let n = smoke_notes(Some(&t), 1).unwrap();
        let m = t.join("private").join("codec-bodies").join("manifest.tsv");
        assert_eq!(n.origin, Origin::Read(m.clone()));
        assert_eq!(ids(&n.inputs), ["notes/one.md", "notes/two.md"]);
        assert_eq!(
            n.line("notes"),
            format!("m6 smoke corpus notes {} 2 inputs 16 bytes", m.display())
        );
        let p = smoke_plan(Some(&t), 1).unwrap();
        assert_eq!(p.origin, Origin::Read(t.join("docs")));
        assert_eq!(ids(&p.inputs), ["docs/x.md#1", "docs/x.md#2"]);
        // A malformed manifest that is present fails the smoke rather than falling back.
        std::fs::write(&m, "body\tbytes\tsha256\tsource\nnotes/one.md\t11\t0\ts\n").unwrap();
        assert!(smoke_notes(Some(&t), 1).unwrap_err().contains("one.md"));
        // An empty tree: both synthetic, and the line names the absent path.
        let e = scratch_dir("m6-empty");
        let n = smoke_notes(Some(&e), 1).unwrap();
        let em = from_checkout(&e).manifest;
        assert_eq!(n.origin, Origin::Synthetic(Some(em.clone())));
        assert_eq!(n.inputs, synthetic_notes(1));
        assert!(n.line("notes").starts_with(&format!(
            "m6 smoke corpus notes synthetic ({} absent) 41 inputs ",
            em.display()
        )));
        let p = smoke_plan(Some(&e), 1).unwrap();
        assert_eq!(p.origin, Origin::Synthetic(Some(e.join("docs"))));
        assert_eq!(p.inputs, synthetic_plan(1));
        let n = smoke_notes(None, 1).unwrap();
        assert_eq!(n.origin, Origin::Synthetic(None));
        assert!(n.line("notes").contains("synthetic (no checkout)"));
        let _ = std::fs::remove_dir_all(&t);
        let _ = std::fs::remove_dir_all(&e);
    }
}
