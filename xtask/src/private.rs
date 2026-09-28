//! Owner-data guards (docs/m0/PLAN.md §2.5, §3.2 item 7 WP-03; AGENTS.md "Data"; authors.md §5 item 5):
//! `xtask private index` writes `/private/MANIFEST.b3`, and `xtask hook pre-commit` (called by
//! `.githooks/pre-commit`) and `xtask gate` check changes against it.
//!
//! **The manifest** (`MANIFEST.b3`, in the main worktree's gitignored `/private/`, never committed) lists every file
//! under `/private/` with its BLAKE3, the 8-word shingle hashes of its text, and a digest of the whole tree:
//!
//! ```text
//! moirai private manifest v2
//! written <unix time, ns>
//! tree <BLAKE3 of the file list, hex>
//! file <BLAKE3 hex> <LF-normalised BLAKE3 hex, or -> <size> <mtime ns> <path relative to /private/, with />
//! ...
//! shingles <n, decimal, zero-padded to 20 digits>
//! <n sorted, distinct shingle hashes, u64 little-endian, 8 n bytes>
//! end <BLAKE3 hex of every byte above this line>
//! ```
//!
//! - **Tree digest.** BLAKE3 over `path NUL size NUL blake3-hex LF` for every file, in byte order of the paths. A
//!   manifest whose digest differs from `/private/` now is stale. The check trusts a file's recorded hash when its
//!   size and mtime are unchanged and the mtime is at least 2 s older than the manifest; any other file is re-hashed.
//! - **LF-normalised hash.** With `* text=auto eol=lf` (`.gitattributes`) git stores a text file's CRLF line ends
//!   as LF, so the staged blob of a copied CRLF file is not byte-equal to the private file. For a file that git's
//!   `text=auto` treats as text (convert.c: no NUL, no lone CR, at most one non-printable byte per 128 printable
//!   ones, a trailing `^Z` not counted) and that has at least one CRLF, the manifest also lists the BLAKE3 of its
//!   bytes with every CRLF replaced by LF, which is the blob git would store; `-` otherwise.
//! - **Shingles.** Text is a file with no NUL in its first 8 KiB. Words are maximal runs of alphanumeric characters,
//!   lower-cased; each word is hashed with xxh3-64, and a shingle is the xxh3-64 of 8 consecutive word hashes
//!   (little-endian), across line ends. Shingles of text already public (the tracked text of `master`, by default)
//!   are left out, so repository text quoted in a private transcript does not refuse ordinary commits. The index
//!   sorts them with bounded memory: runs of at most 4 Mi hashes are sorted, deduplicated and spilled to
//!   `MANIFEST.b3.run<k>.tmp` files beside the manifest, then merged straight into it.
//! - **Checks** over a change (the staged index for the hook; every commit of a range for the gate, merges through
//!   their combined diff): a path under `private/`; a file whose BLAKE3 or LF-normalised BLAKE3 the manifest lists; a
//!   block of added lines containing a listed shingle (a partial copy: one pasted line of 8 words is enough); and, in
//!   `docs/measurements/**` and report files, absolute user paths, user and host names, volume serials, machine
//!   GUIDs, BootIds and process command lines. The shingles of every checked change are matched in one pass over the
//!   manifest, which also verifies its `end` digest, so a run reads the shingle array once.

use crate::diag::Diag;
use crate::git;
use std::collections::{BTreeSet, BinaryHeap};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use xxhash_rust::xxh3::xxh3_64;

pub const MANIFEST: &str = "MANIFEST.b3";
const MAGIC: &str = "moirai private manifest v2";
const MAGIC_PREFIX: &str = "moirai private manifest v";
const CHUNK: usize = 64 * 1024;
const TEXT_PROBE: usize = 8 * 1024;
const RACY_NS: u64 = 2_000_000_000;
pub const SHINGLE_WORDS: usize = 8;
/// The most shingle hashes the index holds in memory at once (32 MiB).
const RUN_CAP: usize = 1 << 22;
/// Width of the zero-padded shingle count, so the count can be written after the array is merged.
const COUNT_WIDTH: usize = 20;

/// The diff options every patch the checks read is produced with: fixed `a/`/`b/` prefixes whatever the user's
/// `diff.noprefix`, `diff.mnemonicPrefix`, `diff.srcPrefix`/`diff.dstPrefix` or `diff.relative` say, no colour, no
/// external diff and no textconv filter (the blob's own bytes are checked), and no rename detection.
pub const PATCH_FLAGS: &[&str] = &[
    "-U0",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-relative",
    "--no-renames",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

// ---------------------------------------------------------------------------------------------------------------
// Shingles

/// Streams text into 8-word shingle hashes.
#[derive(Default)]
pub struct Shingler {
    ring: [u64; SHINGLE_WORDS],
    n: usize,
    word: String,
}

impl Shingler {
    pub fn new() -> Shingler {
        Shingler::default()
    }

    /// Feeds text; calls `out` with each completed shingle.
    pub fn feed(&mut self, s: &str, out: &mut impl FnMut(u64)) {
        for ch in s.chars() {
            if ch.is_alphanumeric() {
                for l in ch.to_lowercase() {
                    self.word.push(l);
                }
            } else {
                self.end_word(out);
            }
        }
    }

    /// Ends the current word (at the end of a text or a block).
    pub fn end_word(&mut self, out: &mut impl FnMut(u64)) {
        if self.word.is_empty() {
            return;
        }
        let h = xxh3_64(self.word.as_bytes());
        self.word.clear();
        self.ring.copy_within(1.., 0);
        self.ring[SHINGLE_WORDS - 1] = h;
        self.n += 1;
        if self.n >= SHINGLE_WORDS {
            let mut buf = [0u8; 8 * SHINGLE_WORDS];
            for (i, w) in self.ring.iter().enumerate() {
                buf[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
            }
            out(xxh3_64(&buf));
        }
    }

    pub fn reset(&mut self) {
        self.n = 0;
        self.word.clear();
    }
}

/// Incremental UTF-8 decoding over byte chunks; invalid bytes act as word separators.
struct Utf8Stream {
    carry: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, chunk: &[u8], f: &mut impl FnMut(&str)) {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(chunk);
        let mut rest: &[u8] = &buf;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    f(s);
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    f(std::str::from_utf8(good).unwrap_or(""));
                    match e.error_len() {
                        Some(n) => {
                            f(" ");
                            rest = &bad[n..];
                        }
                        None => {
                            self.carry = bad.to_vec();
                            break;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Hashing and walking /private/

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The blob git's `text=auto` would store for a file, hashed as it streams (convert.c `gather_stats`,
/// `convert_is_binary`, `crlf_to_git`): every CRLF becomes LF when the file has no NUL, no lone CR and at most one
/// non-printable byte per 128 printable ones (a trailing `^Z` not counted).
#[derive(Default)]
struct LfNorm {
    hasher: blake3::Hasher,
    out: Vec<u8>,
    pending_cr: bool,
    nul: bool,
    lone_cr: bool,
    crlf: u64,
    printable: u64,
    nonprintable: u64,
    last: Option<u8>,
}

impl LfNorm {
    fn update(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        self.out.clear();
        let mut i = 0;
        if self.pending_cr {
            self.pending_cr = false;
            if chunk[0] == b'\n' {
                self.crlf += 1;
                self.out.push(b'\n');
                i = 1;
            } else {
                self.lone_cr = true;
                self.out.push(b'\r');
            }
        }
        while i < chunk.len() {
            let c = chunk[i];
            match c {
                b'\r' => match chunk.get(i + 1) {
                    Some(b'\n') => {
                        self.crlf += 1;
                        self.out.push(b'\n');
                        i += 2;
                        continue;
                    }
                    Some(_) => {
                        self.lone_cr = true;
                        self.out.push(b'\r');
                    }
                    None => self.pending_cr = true,
                },
                b'\n' => self.out.push(c),
                0 => {
                    self.nul = true;
                    self.nonprintable += 1;
                    self.out.push(c);
                }
                8 | 9 | 12 | 27 => {
                    self.printable += 1;
                    self.out.push(c);
                }
                c if c < 32 || c == 127 => {
                    self.nonprintable += 1;
                    self.out.push(c);
                }
                _ => {
                    self.printable += 1;
                    self.out.push(c);
                }
            }
            i += 1;
        }
        self.last = chunk.last().copied();
        self.hasher.update(&self.out);
    }

    /// The normalised BLAKE3, or `None` when git would store the bytes unchanged.
    fn finish(mut self) -> Option<[u8; 32]> {
        if self.pending_cr {
            self.lone_cr = true;
        }
        let nonprintable = if self.last == Some(0x1a) {
            self.nonprintable.saturating_sub(1)
        } else {
            self.nonprintable
        };
        if self.nul || self.lone_cr || self.crlf == 0 || (self.printable >> 7) < nonprintable {
            return None;
        }
        Some(*self.hasher.finalize().as_bytes())
    }
}

/// A file's hashes.
struct FileHashes {
    b3: [u8; 32],
    lf: Option<[u8; 32]>,
    size: u64,
}

/// BLAKE3 of a file, its LF-normalised BLAKE3, and its shingles (into `shingles`) when it is text.
fn hash_file(path: &Path, shingles: Option<&mut dyn FnMut(u64)>) -> Result<FileHashes, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut lf = LfNorm::default();
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    let mut text: Option<bool> = None;
    let mut sh = Shingler::new();
    let mut dec = Utf8Stream { carry: Vec::new() };
    let mut sink = shingles;
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        hasher.update(chunk);
        lf.update(chunk);
        if text.is_none() {
            let probe = &chunk[..chunk.len().min(TEXT_PROBE)];
            text = Some(!probe.contains(&0));
        }
        if text == Some(true)
            && let Some(out) = sink.as_mut()
        {
            dec.push(chunk, &mut |s| sh.feed(s, &mut |h| out(h)));
        }
        size += n as u64;
    }
    if text == Some(true)
        && let Some(out) = sink.as_mut()
    {
        sh.end_word(&mut |h| out(h));
    }
    Ok(FileHashes {
        b3: *hasher.finalize().as_bytes(),
        lf: lf.finish(),
        size,
    })
}

fn mtime_ns(md: &std::fs::Metadata) -> u64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Whether a file at the top of `/private/` is the manifest or one of its temporary files.
fn manifest_file(name: &str) -> bool {
    name.starts_with(MANIFEST)
}

/// Every regular file under `dir` as `(relative path with /, absolute path, metadata)`, sorted by path bytes.
/// Symbolic links are not followed and are reported as errors, so nothing under `/private/` escapes the manifest.
fn walk(dir: &Path) -> Result<Vec<(String, PathBuf, std::fs::Metadata)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((abs, rel)) = stack.pop() {
        for e in std::fs::read_dir(&abs).map_err(|e| format!("{}: {e}", abs.display()))? {
            let e = e.map_err(|e| e.to_string())?;
            let name = e
                .file_name()
                .into_string()
                .map_err(|n| format!("{}: a file name that is not UTF-8: {n:?}", abs.display()))?;
            if name.contains('\n') {
                return Err(format!("{}: a file name with a line feed", abs.display()));
            }
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_symlink() {
                return Err(format!(
                    "{}: symbolic links are not indexed; replace it with the file",
                    e.path().display()
                ));
            }
            if ft.is_dir() {
                stack.push((e.path(), r));
            } else if rel.is_empty() && manifest_file(&name) {
                continue;
            } else {
                let md = e.metadata().map_err(|e| e.to_string())?;
                out.push((r, e.path(), md));
            }
        }
    }
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    Ok(out)
}

fn tree_digest<'a>(entries: impl Iterator<Item = (&'a str, u64, &'a [u8; 32])>) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for (p, size, b3) in entries {
        h.update(p.as_bytes());
        h.update(b"\0");
        h.update(size.to_string().as_bytes());
        h.update(b"\0");
        h.update(hex(b3).as_bytes());
        h.update(b"\n");
    }
    *h.finalize().as_bytes()
}

// ---------------------------------------------------------------------------------------------------------------
// The index: bounded-memory sorting of shingles

/// Sorted, deduplicated runs of shingle hashes: at most `cap` in memory, the rest spilled to temporary files.
struct Runs {
    buf: Vec<u64>,
    cap: usize,
    dir: PathBuf,
    files: Vec<PathBuf>,
    error: Option<String>,
}

impl Runs {
    fn new(dir: &Path, cap: usize) -> Runs {
        Runs {
            buf: Vec::with_capacity(cap.min(1 << 16)),
            cap: cap.max(2),
            dir: dir.to_path_buf(),
            files: Vec::new(),
            error: None,
        }
    }

    fn push(&mut self, h: u64) {
        self.buf.push(h);
        if self.buf.len() >= self.cap {
            self.buf.sort_unstable();
            self.buf.dedup();
            // Spill once deduplication no longer frees half the buffer, so each hash is sorted O(1) times.
            if self.buf.len() > self.cap / 2
                && let Err(e) = self.spill()
            {
                self.error.get_or_insert(e);
                self.buf.clear();
            }
        }
    }

    fn spill(&mut self) -> Result<(), String> {
        let p = self
            .dir
            .join(format!("{MANIFEST}.run{}.tmp", self.files.len()));
        let f = std::fs::File::create(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        self.files.push(p.clone());
        let mut w = BufWriter::with_capacity(CHUNK, f);
        for h in &self.buf {
            w.write_all(&h.to_le_bytes())
                .map_err(|e| format!("{}: {e}", p.display()))?;
        }
        w.flush().map_err(|e| format!("{}: {e}", p.display()))?;
        self.buf.clear();
        Ok(())
    }

    /// Merges every run and the in-memory tail in ascending order, calling `out` once per distinct hash.
    fn merge(&mut self, out: &mut dyn FnMut(u64) -> Result<(), String>) -> Result<(), String> {
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        self.buf.sort_unstable();
        self.buf.dedup();
        let mut readers = Vec::with_capacity(self.files.len());
        for p in &self.files {
            let f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
            readers.push(BufReader::with_capacity(CHUNK, f));
        }
        let next = |r: &mut BufReader<std::fs::File>| -> Result<Option<u64>, String> {
            let mut b = [0u8; 8];
            match r.read_exact(&mut b) {
                Ok(()) => Ok(Some(u64::from_le_bytes(b))),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        };
        // Source `readers.len()` is the in-memory tail.
        let tail = readers.len();
        let mut heap = BinaryHeap::new();
        for (i, r) in readers.iter_mut().enumerate() {
            if let Some(v) = next(r)? {
                heap.push(std::cmp::Reverse((v, i)));
            }
        }
        let mut ti = 0;
        if let Some(&v) = self.buf.first() {
            heap.push(std::cmp::Reverse((v, tail)));
            ti = 1;
        }
        let mut last: Option<u64> = None;
        while let Some(std::cmp::Reverse((v, i))) = heap.pop() {
            if last != Some(v) {
                out(v)?;
                last = Some(v);
            }
            let n = if i == tail {
                let n = self.buf.get(ti).copied();
                ti += 1;
                n
            } else {
                next(&mut readers[i])?
            };
            if let Some(n) = n {
                heap.push(std::cmp::Reverse((n, i)));
            }
        }
        Ok(())
    }
}

impl Drop for Runs {
    fn drop(&mut self) {
        for p in &self.files {
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The manifest

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub hash: [u8; 32],
    /// The LF-normalised BLAKE3, for a text file with CRLF line ends.
    pub lf: Option<[u8; 32]>,
    pub size: u64,
    pub mtime_ns: u64,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub written_ns: u64,
    pub tree: [u8; 32],
    pub files: Vec<FileEntry>,
    pub shingles: u64,
}

pub struct IndexStats {
    pub files: usize,
    pub text_files: usize,
    pub shingles: u64,
    pub public_removed: u64,
    pub runs: usize,
}

/// `xtask private index`: rebuilds `<private_dir>/MANIFEST.b3`. `public` holds the shingles of already-public text,
/// sorted and distinct.
pub fn index(private_dir: &Path, public: &[u64]) -> Result<IndexStats, String> {
    index_with(private_dir, public, RUN_CAP)
}

fn index_with(private_dir: &Path, public: &[u64], cap: usize) -> Result<IndexStats, String> {
    if !private_dir.is_dir() {
        return Err(format!("{} is not a directory", private_dir.display()));
    }
    let entries = walk(private_dir)?;
    let mut runs = Runs::new(private_dir, cap);
    let mut files = Vec::with_capacity(entries.len());
    let mut text_files = 0;
    for (rel, abs, md) in &entries {
        let mut any = false;
        let h = {
            let mut sink = |s: u64| {
                any = true;
                runs.push(s);
            };
            hash_file(abs, Some(&mut sink))?
        };
        if let Some(e) = runs.error.take() {
            return Err(e);
        }
        text_files += usize::from(any);
        files.push(FileEntry {
            hash: h.b3,
            lf: h.lf,
            size: h.size,
            mtime_ns: mtime_ns(md),
            path: rel.clone(),
        });
    }
    let tree = tree_digest(files.iter().map(|f| (f.path.as_str(), f.size, &f.hash)));
    let written = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = private_dir.join(format!("{MANIFEST}.tmp"));
    let dst = private_dir.join(MANIFEST);
    let io = |e: std::io::Error| format!("{}: {e}", tmp.display());
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(io)?;
    let mut w = BufWriter::with_capacity(CHUNK, file);
    let mut head = format!("{MAGIC}\nwritten {written}\ntree {}\n", hex(&tree));
    for f in &files {
        head.push_str(&format!(
            "file {} {} {} {} {}\n",
            hex(&f.hash),
            f.lf.map_or_else(|| "-".to_string(), |h| hex(&h)),
            f.size,
            f.mtime_ns,
            f.path
        ));
    }
    head.push_str("shingles ");
    let count_at = head.len() as u64;
    head.push_str(&"0".repeat(COUNT_WIDTH));
    head.push('\n');
    w.write_all(head.as_bytes()).map_err(io)?;
    drop(head);
    // Merge the runs into the array, leaving out public shingles.
    let (mut count, mut removed, mut pi) = (0u64, 0u64, 0usize);
    let nruns = runs.files.len();
    runs.merge(&mut |v| {
        while pi < public.len() && public[pi] < v {
            pi += 1;
        }
        if pi < public.len() && public[pi] == v {
            removed += 1;
            return Ok(());
        }
        count += 1;
        w.write_all(&v.to_le_bytes()).map_err(io)
    })?;
    drop(runs);
    let mut file = w.into_inner().map_err(|e| io(e.into_error()))?;
    file.seek(SeekFrom::Start(count_at)).map_err(io)?;
    file.write_all(format!("{count:0w$}", w = COUNT_WIDTH).as_bytes())
        .map_err(io)?;
    // The `end` digest over everything written, read back in one pass.
    file.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    file.seek(SeekFrom::End(0)).map_err(io)?;
    file.write_all(format!("end {}\n", hex(h.finalize().as_bytes())).as_bytes())
        .map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    std::fs::rename(&tmp, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(IndexStats {
        files: files.len(),
        text_files,
        shingles: count,
        public_removed: removed,
        runs: nruns,
    })
}

/// One text line of the manifest, hashed into `hasher`, without its line feed.
fn read_line<R: BufRead>(
    r: &mut R,
    hasher: &mut blake3::Hasher,
    path: &Path,
) -> Result<String, String> {
    let mut line = Vec::new();
    r.read_until(b'\n', &mut line)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !line.ends_with(b"\n") {
        return Err(format!("{}: truncated manifest", path.display()));
    }
    hasher.update(&line);
    line.pop();
    String::from_utf8(line)
        .map_err(|_| format!("{}: a manifest line that is not UTF-8", path.display()))
}

/// Reads the manifest's text part (every line up to `shingles <n>`), leaving the reader at the shingle array.
fn read_head<R: BufRead>(
    path: &Path,
    r: &mut R,
    h: &mut blake3::Hasher,
) -> Result<Manifest, String> {
    let bad = |m: &str| format!("{}: {m}", path.display());
    let next = |r: &mut R, h: &mut blake3::Hasher| read_line(r, h, path);
    let magic = next(r, h)?;
    if magic != MAGIC {
        return Err(bad(if magic.starts_with(MAGIC_PREFIX) {
            "written by another version of xtask: re-run cargo xtask private index"
        } else {
            "not a moirai private manifest"
        }));
    }
    let written_ns = next(r, h)?
        .strip_prefix("written ")
        .and_then(|s| s.parse::<u128>().ok())
        .map(|n| u64::try_from(n).unwrap_or(u64::MAX))
        .ok_or_else(|| bad("missing 'written'"))?;
    let tree = next(r, h)?
        .strip_prefix("tree ")
        .and_then(unhex32)
        .ok_or_else(|| bad("missing 'tree'"))?;
    let mut files = Vec::new();
    loop {
        let l = next(r, h)?;
        if let Some(rest) = l.strip_prefix("file ") {
            let mut it = rest.splitn(5, ' ');
            let (Some(hx), Some(lf), Some(sz), Some(mt), Some(p)) =
                (it.next(), it.next(), it.next(), it.next(), it.next())
            else {
                return Err(bad("malformed file line"));
            };
            files.push(FileEntry {
                hash: unhex32(hx).ok_or_else(|| bad("malformed file hash"))?,
                lf: match lf {
                    "-" => None,
                    x => Some(unhex32(x).ok_or_else(|| bad("malformed LF-normalised hash"))?),
                },
                size: sz.parse().map_err(|_| bad("malformed size"))?,
                mtime_ns: mt.parse().map_err(|_| bad("malformed mtime"))?,
                path: p.to_string(),
            });
        } else if let Some(n) = l.strip_prefix("shingles ") {
            let shingles = n
                .parse::<u64>()
                .map_err(|_| bad("malformed shingle count"))?;
            return Ok(Manifest {
                written_ns,
                tree,
                files,
                shingles,
            });
        } else {
            return Err(bad(&format!("unexpected line '{l}'")));
        }
    }
}

/// Reads the manifest's text part only; the `end` digest is verified by the full pass ([`read_manifest`]).
pub fn read_header(path: &Path) -> Result<Manifest, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut r = BufReader::with_capacity(CHUNK, f);
    read_head(path, &mut r, &mut blake3::Hasher::new())
}

/// Reads the whole manifest; `on_shingle` receives every shingle hash in order. The trailing `end` digest is
/// verified in the same pass.
pub fn read_manifest(path: &Path, on_shingle: &mut dyn FnMut(u64)) -> Result<Manifest, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut r = BufReader::with_capacity(CHUNK, f);
    let mut h = blake3::Hasher::new();
    let m = read_head(path, &mut r, &mut h)?;
    let bad = |m: &str| format!("{}: {m}", path.display());
    let mut buf = vec![0u8; CHUNK];
    let mut left = m
        .shingles
        .checked_mul(8)
        .ok_or_else(|| bad("shingle count too large"))?;
    let mut prev: Option<u64> = None;
    while left > 0 {
        let want = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
        r.read_exact(&mut buf[..want])
            .map_err(|_| bad("truncated shingle array"))?;
        h.update(&buf[..want]);
        for c in buf[..want].as_chunks::<8>().0 {
            let v = u64::from_le_bytes(*c);
            if prev.is_some_and(|p| p >= v) {
                return Err(bad("shingle array not sorted"));
            }
            prev = Some(v);
            on_shingle(v);
        }
        left -= want as u64;
    }
    let expect = hex(h.finalize().as_bytes());
    let mut last = String::new();
    r.read_line(&mut last).map_err(|e| e.to_string())?;
    if last.trim_end() != format!("end {expect}") {
        return Err(bad(
            "the 'end' digest does not match: the manifest is damaged; re-run cargo xtask private index",
        ));
    }
    Ok(m)
}

/// Recomputes `/private/`'s tree digest, re-hashing only files whose size or mtime changed (or are racy). Every file
/// is stat'ed on each call: a cache keyed on directory mtimes would miss a file rewritten in place, whose directory
/// mtime does not change.
pub fn current_tree(private_dir: &Path, m: &Manifest) -> Result<[u8; 32], String> {
    let known: std::collections::BTreeMap<&str, &FileEntry> =
        m.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let entries = walk(private_dir)?;
    let mut rows: Vec<(String, u64, [u8; 32])> = Vec::with_capacity(entries.len());
    for (rel, abs, md) in entries {
        let mt = mtime_ns(&md);
        let size = md.len();
        let hash = match known.get(rel.as_str()) {
            Some(f)
                if f.size == size
                    && f.mtime_ns == mt
                    && mt.saturating_add(RACY_NS) <= m.written_ns =>
            {
                f.hash
            }
            _ => hash_file(&abs, None)?.b3,
        };
        rows.push((rel, size, hash));
    }
    Ok(tree_digest(
        rows.iter().map(|(p, s, h)| (p.as_str(), *s, h)),
    ))
}

// ---------------------------------------------------------------------------------------------------------------
// Changes to check

/// A changed file of a change set: path, new blob id and new mode.
#[derive(Clone, Debug)]
pub struct Changed {
    pub path: String,
    pub blob: String,
    pub mode: String,
    /// The status letters (one per parent in a combined diff).
    pub status: String,
}

/// Parses `git diff --raw -z` or `diff-tree --raw -z` output, `:old new oldid newid status NUL path NUL`, and the
/// combined form of a merge (`diff-tree -c --raw`), `::m1 m2 new id1 id2 newid statuses NUL path NUL`, with one
/// colon per parent.
pub fn parse_raw_z(out: &[u8]) -> Vec<Changed> {
    let mut v = Vec::new();
    let mut it = out.split(|&c| c == 0).filter(|s| !s.is_empty());
    while let Some(meta) = it.next() {
        let meta = String::from_utf8_lossy(meta);
        let parents = meta.bytes().take_while(|&c| c == b':').count();
        let Some(path) = it.next() else { break };
        let f: Vec<&str> = meta[parents..].split(' ').collect();
        if parents == 0 || f.len() < 2 * (parents + 1) + 1 {
            continue;
        }
        v.push(Changed {
            path: String::from_utf8_lossy(path).into_owned(),
            blob: f[2 * parents + 1].to_string(),
            mode: f[parents].to_string(),
            status: f[2 * (parents + 1)].to_string(),
        });
    }
    v
}

/// Undoes git's C-style path quoting (`"a\tb"`).
pub fn unquote(p: &str) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return p.to_string();
    };
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            let c = b[i + 1];
            match c {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'0'..=b'7' if i + 3 < b.len() => {
                    let oct = std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("0");
                    out.push(u8::from_str_radix(oct, 8).unwrap_or(b'?'));
                    i += 4;
                    continue;
                }
                other => out.push(other),
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Streams a `-U0` unified diff (or the combined diff of a merge, `--cc`) made with [`PATCH_FLAGS`] and calls
/// `on_added(path, new line number, text, starts_block)` for every added line: in a combined diff, a line added
/// relative to every parent (every prefix column `+`). A block is a run of consecutive added lines in one hunk.
///
/// Fails closed: a `+++` header that is neither `/dev/null` nor `b/<path>`, or an added line with no path, is an
/// error, never a skipped line.
pub fn each_added_line(
    r: impl BufRead,
    on_added: &mut dyn FnMut(&str, u32, &str, bool),
) -> Result<(), String> {
    let mut path: Option<String> = None;
    let mut in_hunk = false;
    let mut cols = 1usize;
    let mut next = 0u32;
    let mut prev_added = false;
    let mut r = r;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = r.read_until(b'\n', &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if buf.ends_with(b"\n") {
            buf.pop();
        }
        if buf.ends_with(b"\r") {
            buf.pop();
        }
        let line = String::from_utf8_lossy(&buf);
        if line.starts_with("diff --git ") || line.starts_with("diff --cc ") {
            path = None;
            in_hunk = false;
            prev_added = false;
            continue;
        }
        let ats = line.bytes().take_while(|&c| c == b'@').count();
        if ats >= 2 && line.as_bytes().get(ats) == Some(&b' ') {
            in_hunk = true;
            prev_added = false;
            cols = ats - 1;
            let plus = line[ats + 1..]
                .split(' ')
                .take_while(|t| !t.starts_with('@'))
                .filter(|t| t.starts_with('+'))
                .last()
                .unwrap_or("+0");
            next = plus[1..]
                .split(',')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            continue;
        }
        if !in_hunk {
            if let Some(p) = line.strip_prefix("+++ ") {
                let p = p.trim_end_matches('\t');
                if p == "/dev/null" {
                    path = None;
                } else {
                    let u = unquote(p);
                    path = Some(u.strip_prefix("b/").map(str::to_string).ok_or_else(|| {
                        format!(
                            "diff header '+++ {p}' names no b/<path>: refusing to skip its lines"
                        )
                    })?);
                }
            }
            continue;
        }
        if line.starts_with('\\') {
            continue;
        }
        let b = line.as_bytes();
        let prefix = &b[..cols.min(b.len())];
        if prefix.contains(&b'-') {
            prev_added = false;
            continue;
        }
        if prefix.len() == cols && prefix.iter().all(|&c| c == b'+') {
            let Some(p) = &path else {
                return Err(format!(
                    "an added line in a hunk with no b/<path> header (line {next}): refusing to skip it"
                ));
            };
            on_added(p, next, &line[cols..], !prev_added);
            prev_added = true;
        } else {
            prev_added = false;
        }
        next = next.saturating_add(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Report scrub

/// Whether a path is a measurement report whose added lines are scrubbed.
pub fn is_report_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("docs/measurements/")
        || (lower.starts_with("docs/")
            && lower
                .rsplit('/')
                .next()
                .is_some_and(|f| f.contains("report")))
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

/// Hex runs of a line as `(start, end)`, split at non-hex bytes.
fn hex_runs(b: &[u8]) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if is_hex(b[i]) && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let s = i;
            while i < b.len() && is_hex(b[i]) {
                i += 1;
            }
            if i == b.len() || !b[i].is_ascii_alphanumeric() {
                v.push((s, i));
            }
        } else {
            i += 1;
        }
    }
    v
}

fn has_guid(b: &[u8]) -> bool {
    let runs = hex_runs(b);
    runs.windows(5).any(|w| {
        let lens: Vec<usize> = w.iter().map(|(s, e)| e - s).collect();
        lens == [8, 4, 4, 4, 12]
            && w.windows(2)
                .all(|p| p[0].1 + 1 == p[1].0 && b[p[0].1] == b'-')
    })
}

/// Whether `word` occurs in `line` as a whole word (case-insensitive; the characters around it are not
/// alphanumeric, Unicode included).
fn has_word_ci(line: &str, word: &str) -> bool {
    let l = line.to_lowercase();
    let w = word.to_lowercase();
    if w.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = l[from..].find(&w) {
        let s = from + i;
        let e = s + w.len();
        let before = l[..s]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = l[e..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = s + l[s..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

fn user_path(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let b = l.as_bytes();
    let placeholder = |rest: &str| {
        rest.is_empty()
            || matches!(rest.as_bytes()[0], b'<' | b'%' | b'$' | b'{' | b'*' | b'.')
            || rest.starts_with("user")
                && !rest[4..].starts_with(|c: char| c.is_ascii_alphanumeric())
    };
    // Windows: X:\Users\name, X:/Users/name, \\?\X:\Users, /c/Users/name, /mnt/c/users/name.
    for (i, _) in l.match_indices("users") {
        let before = &l[..i];
        let sep_before = before.ends_with('\\') || before.ends_with('/');
        if !sep_before {
            continue;
        }
        let root = before.trim_end_matches(['\\', '/']);
        let drive = root.len() >= 2
            && root.as_bytes()[root.len() - 1] == b':'
            && root.as_bytes()[root.len() - 2].is_ascii_alphabetic();
        let msys = root.len() >= 2
            && root.as_bytes()[root.len() - 2] == b'/'
            && root.as_bytes()[root.len() - 1].is_ascii_alphabetic();
        let mac = root.is_empty()
            || root.ends_with(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '(' | '=' | '`')
            });
        if !(drive || msys || mac) {
            continue;
        }
        let after = &l[i + 5..];
        if !(after.starts_with('\\') || after.starts_with('/')) {
            continue;
        }
        let name = after.trim_start_matches(['\\', '/']);
        if !placeholder(name)
            && name
                .as_bytes()
                .first()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            return true;
        }
    }
    for (i, _) in l.match_indices("/home/") {
        let ok_before = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let name = &l[i + 6..];
        if ok_before
            && !placeholder(name)
            && name
                .as_bytes()
                .first()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            return true;
        }
    }
    false
}

/// Whether a BootId *value* follows one of the BootId keywords: after the keyword, optional separators (`=`, `:`,
/// `is`, quotes, spaces), a decimal number (Windows' `BootId` is a DWORD boot count, [OS/mapping-appendix]), a hex
/// number, or a GUID (Linux `boot_id`, macOS `kern.bootsessionuuid`). Aggregate phrasing such as "BootId unchanged in
/// 5 of 5 reboots" names no value.
fn boot_value(lower: &str) -> bool {
    for k in ["bootid", "boot_id", "boot id", "boot-id"] {
        for (i, _) in lower.match_indices(k) {
            if lower[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            let mut rest = &lower[i + k.len()..];
            loop {
                let t = rest.trim_start_matches([' ', '\t', '=', ':', '"', '\'', '`', '(', '[']);
                let t = t.strip_prefix("is ").unwrap_or(t);
                if t.len() == rest.len() {
                    break;
                }
                rest = t;
            }
            let tok: &str = rest
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '{'))
                .next()
                .unwrap_or("");
            let tok = tok.trim_start_matches('{');
            let prefixed = tok.strip_prefix("0x");
            let t = prefixed.unwrap_or(tok);
            let all_hex = !t.is_empty() && t.bytes().all(|c| c.is_ascii_hexdigit());
            let numeric = !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
            let hexy = all_hex && (prefixed.is_some() || t.len() >= 8);
            if numeric || hexy || has_guid(tok.as_bytes()) {
                return true;
            }
        }
    }
    false
}

/// The kinds of machine- or user-identifying data an added report line carries. `names` are the user and host
/// names of this machine.
pub fn scrub_line(line: &str, names: &[String]) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    let b = line.as_bytes();
    let lower = line.to_ascii_lowercase();
    if user_path(line) {
        kinds.push("an absolute user path");
    }
    if names
        .iter()
        .any(|n| n.chars().count() >= 3 && has_word_ci(line, n))
    {
        kinds.push("a user or host name");
    }
    let runs = hex_runs(b);
    let serial_ctx = lower.contains("serial");
    let dashed_serial = runs.windows(2).any(|w| {
        w[0].1 - w[0].0 == 4
            && w[1].1 - w[1].0 == 4
            && w[0].1 + 1 == w[1].0
            && b[w[0].1] == b'-'
            && b[w[0].0..w[1].1].iter().any(|c| c.is_ascii_alphabetic())
    });
    if dashed_serial || (serial_ctx && runs.iter().any(|(s, e)| e - s >= 8)) {
        kinds.push("a volume serial");
    }
    if has_guid(b) {
        kinds.push("a machine GUID");
    }
    // A boot hash (BLAKE3-128, 32 hex digits, [OS/proc §4.2]) is flagged only beside a boot keyword: other 128-bit
    // hex values (fixture digests, ids) are ordinary report content.
    let boot_hash = lower.contains("boot") && runs.iter().any(|(s, e)| e - s == 32);
    if boot_value(&lower) || boot_hash {
        kinds.push("a BootId");
    }
    let cmd_ctx = ["commandline", "command_line", "cmdline", "command line:"]
        .iter()
        .any(|k| lower.contains(k));
    let exe_args = lower.match_indices(".exe").any(|(i, _)| {
        let rest = &lower[i + 4..];
        let rest = rest.trim_start_matches('"');
        rest.starts_with(' ') && rest.trim_start().starts_with(['-', '/'])
    });
    if cmd_ctx || exe_args {
        kinds.push("a process command line");
    }
    kinds
}

/// The user and host names of this machine, from the environment.
pub fn machine_names() -> Vec<String> {
    let mut v: Vec<String> = [
        "USERNAME",
        "USER",
        "LOGNAME",
        "COMPUTERNAME",
        "HOSTNAME",
        "USERDOMAIN",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok())
    .map(|s| s.trim().to_string())
    .filter(|s| s.chars().count() >= 3)
    .collect();
    v.sort();
    v.dedup();
    v
}

// ---------------------------------------------------------------------------------------------------------------
// The checks over change sets

/// What to check a change set against.
pub struct Guards<'a> {
    /// `/private/` and its manifest header, when present.
    pub manifest: Option<(&'a Path, &'a Manifest, PathBuf)>,
    pub names: &'a [String],
}

/// Shingles of added lines, collected over one or more change sets and matched against the manifest in one pass.
#[derive(Default)]
pub struct Hits {
    /// `(shingle, index into places, line)`.
    hits: Vec<(u64, u32, u32)>,
    /// `(label, path)` of each block's file.
    places: Vec<(String, String)>,
}

impl Hits {
    /// Reads the whole manifest once, verifying its `end` digest even when nothing was added, and reports every
    /// added line that repeats a listed shingle.
    pub fn resolve(mut self, mpath: &Path, out: &mut Vec<Diag>) -> Result<(), String> {
        self.hits.sort_unstable();
        let mut found: Vec<(u32, u32)> = Vec::new();
        let mut k = 0;
        let hits = &self.hits;
        read_manifest(mpath, &mut |v| {
            while k < hits.len() && hits[k].0 < v {
                k += 1;
            }
            let mut j = k;
            while j < hits.len() && hits[j].0 == v {
                found.push((hits[j].1, hits[j].2));
                j += 1;
            }
        })?;
        found.sort_unstable();
        found.dedup();
        for (pi, line) in found {
            let (label, p) = &self.places[pi as usize];
            out.push(Diag {
                line: Some(line),
                ..Diag::path(
                    "private",
                    p,
                    format!("{label}: an added line repeats an 8-word sequence of a file under /private/ (a partial copy)"),
                )
            });
        }
        Ok(())
    }
}

/// Checks one change set. `changed` lists its files; `blob_hashes` gives each changed blob's BLAKE3; `diff` is the
/// set's patch ([`PATCH_FLAGS`]; combined for a merge). `label` prefixes each finding (a commit id, or "staged").
/// The shingles of added lines go to `hits`, which [`Hits::resolve`] matches after the last change set.
pub fn check_changes(
    label: &str,
    changed: &[Changed],
    blob_hashes: &[(String, [u8; 32])],
    diff: impl BufRead,
    g: &Guards<'_>,
    hits: &mut Hits,
) -> Result<Vec<Diag>, String> {
    let mut out = Vec::new();
    for c in changed {
        if c.path.to_ascii_lowercase().starts_with("private/") {
            out.push(Diag::path(
                "private",
                &c.path,
                format!("{label}: owner data under private/ is never committed"),
            ));
        }
    }
    if let Some((_, m, _)) = &g.manifest {
        let listed: BTreeSet<[u8; 32]> = m
            .files
            .iter()
            .flat_map(|f| std::iter::once(f.hash).chain(f.lf))
            .collect();
        for (path, h) in blob_hashes {
            if listed.contains(h) {
                out.push(Diag::path("private", path, format!("{label}: this file is a copy of a file under /private/ (its BLAKE3 is listed)")));
            }
        }
    }
    // Added lines: shingles and the report scrub.
    let mut sh = Shingler::new();
    let mut cur: Option<u32> = None;
    let want_shingles = g.manifest.is_some();
    each_added_line(diff, &mut |path, line, text, start| {
        let same = cur.is_some_and(|i| hits.places[i as usize].1 == path);
        if start || !same {
            sh.reset();
            if !same {
                hits.places.push((label.to_string(), path.to_string()));
                cur = Some(u32::try_from(hits.places.len() - 1).unwrap_or(u32::MAX));
            }
        }
        let pi = cur.unwrap_or(0);
        if want_shingles {
            sh.feed(text, &mut |h| hits.hits.push((h, pi, line)));
            sh.feed(" ", &mut |h| hits.hits.push((h, pi, line)));
        }
        if is_report_path(path) {
            for k in scrub_line(text, g.names) {
                out.push(Diag {
                    line: Some(line),
                    ..Diag::path(
                        "report-scrub",
                        path,
                        format!(
                            "{label}: {k} in a measurement report (PLAN WP-03; aggregates only)"
                        ),
                    )
                });
            }
        }
    })?;
    Ok(out)
}

/// Reads blob contents through one `git cat-file --batch` process, calling `f(index, Some(chunk))` for each chunk of
/// each blob and `f(index, None)` at its end.
fn cat_blobs(
    repo: &Path,
    ids: Vec<String>,
    f: &mut dyn FnMut(usize, Option<&[u8]>),
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let n = ids.len();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("git cat-file: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        let mut w = BufWriter::new(&mut stdin);
        for id in ids {
            w.write_all(id.as_bytes())?;
            w.write_all(b"\n")?;
        }
        w.flush()
    });
    let stdout = child.stdout.take().ok_or("git cat-file: no stdout")?;
    let mut r = BufReader::with_capacity(CHUNK, stdout);
    let mut buf = vec![0u8; CHUNK];
    let res = (|| -> Result<(), String> {
        for i in 0..n {
            let mut head = String::new();
            r.read_line(&mut head).map_err(|e| e.to_string())?;
            let fields: Vec<&str> = head.split_whitespace().collect();
            if fields.len() != 3 {
                return Err(format!("git cat-file: unexpected reply '{}'", head.trim()));
            }
            let mut left: u64 = fields[2]
                .parse()
                .map_err(|_| format!("git cat-file: bad size in '{}'", head.trim()))?;
            while left > 0 {
                let k = usize::try_from(left.min(CHUNK as u64)).unwrap_or(CHUNK);
                r.read_exact(&mut buf[..k]).map_err(|e| e.to_string())?;
                f(i, Some(&buf[..k]));
                left -= k as u64;
            }
            let mut nl = [0u8; 1];
            r.read_exact(&mut nl).map_err(|e| e.to_string())?;
            f(i, None);
        }
        Ok(())
    })();
    drop(r);
    let w = writer
        .join()
        .map_err(|_| "git cat-file writer panicked".to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    res?;
    w.map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("git cat-file exited with {status}"));
    }
    Ok(())
}

/// BLAKE3 of the changed blobs (gitlinks and deletions left out).
pub fn blob_hashes(repo: &Path, changed: &[Changed]) -> Result<Vec<(String, [u8; 32])>, String> {
    let wanted: Vec<&Changed> = changed
        .iter()
        .filter(|c| c.mode != "160000" && c.blob.bytes().any(|b| b != b'0'))
        .collect();
    let mut out = Vec::with_capacity(wanted.len());
    let mut cur = blake3::Hasher::new();
    cat_blobs(
        repo,
        wanted.iter().map(|c| c.blob.clone()).collect(),
        &mut |i, chunk| match chunk {
            Some(c) => {
                cur.update(c);
            }
            None => {
                out.push((wanted[i].path.clone(), *cur.finalize().as_bytes()));
                cur = blake3::Hasher::new();
            }
        },
    )?;
    Ok(out)
}

/// Collects shingle hashes into a sorted, distinct vector, re-sorting only when the vector has doubled since the
/// last deduplication (amortised O(N log N)).
struct Collector {
    v: Vec<u64>,
    next_sort: usize,
}

impl Collector {
    fn push(&mut self, h: u64) {
        self.v.push(h);
        if self.v.len() >= self.next_sort {
            self.v.sort_unstable();
            self.v.dedup();
            self.next_sort = (2 * self.v.len()).max(RUN_CAP);
        }
    }
    fn finish(mut self) -> Vec<u64> {
        self.v.sort_unstable();
        self.v.dedup();
        self.v
    }
}

/// Shingles of the tracked text files of `rev` (the public text left out of the manifest), sorted and distinct.
pub fn public_shingles(repo: &Path, rev: &str) -> Result<Vec<u64>, String> {
    let ls = git::run_bytes(repo, &["ls-tree", "-r", "-z", "--full-tree", rev])?;
    let mut ids = Vec::new();
    for rec in ls.split(|&c| c == 0).filter(|s| !s.is_empty()) {
        let rec = String::from_utf8_lossy(rec);
        let Some((meta, _path)) = rec.split_once('\t') else {
            continue;
        };
        let f: Vec<&str> = meta.split(' ').collect();
        if f.len() == 3 && f[1] == "blob" {
            ids.push(f[2].to_string());
        }
    }
    let mut out = Collector {
        v: Vec::new(),
        next_sort: RUN_CAP,
    };
    let mut sh = Shingler::new();
    let mut dec = Utf8Stream { carry: Vec::new() };
    let mut text: Option<bool> = None;
    cat_blobs(repo, ids, &mut |_, chunk| match chunk {
        Some(chunk) => {
            if text.is_none() {
                text = Some(!chunk[..chunk.len().min(TEXT_PROBE)].contains(&0));
            }
            if text == Some(true) {
                dec.push(chunk, &mut |s| sh.feed(s, &mut |h| out.push(h)));
            }
        }
        None => {
            if text == Some(true) {
                sh.end_word(&mut |h| out.push(h));
            }
            sh.reset();
            dec.carry.clear();
            text = None;
        }
    })?;
    Ok(out.finish())
}

/// Loads the manifest header of `private_dir` and checks that it is current. `Ok(None)`: no manifest. The body is
/// read (and its `end` digest verified) once, by [`Hits::resolve`].
pub fn load_current(private_dir: &Path) -> Result<Option<(Manifest, PathBuf)>, String> {
    let mpath = private_dir.join(MANIFEST);
    if !mpath.is_file() {
        return Ok(None);
    }
    let m = read_header(&mpath)?;
    let now = current_tree(private_dir, &m)?;
    if now != m.tree {
        return Err(format!(
            "{} is stale: /private/ changed since it was written (tree digest {} != {}); run cargo xtask private index",
            mpath.display(),
            &hex(&now)[..16],
            &hex(&m.tree)[..16]
        ));
    }
    Ok(Some((m, mpath)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shingles_of(s: &str) -> Vec<u64> {
        let mut sh = Shingler::new();
        let mut v = Vec::new();
        sh.feed(s, &mut |h| v.push(h));
        sh.end_word(&mut |h| v.push(h));
        v
    }

    #[test]
    fn shingles_ignore_case_punctuation_and_line_breaks() {
        let a = shingles_of("The quick brown fox jumps over the lazy dog today");
        assert_eq!(a.len(), 3);
        let b = shingles_of("the QUICK, brown fox —\njumps over the lazy dog; today!");
        assert_eq!(a, b);
        assert!(shingles_of("only seven words are in this line").is_empty());
        let c = shingles_of("Пример строки на русском языке для проверки хешей слов");
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn utf8_stream_across_chunks() {
        let text = "слово ".repeat(20);
        let bytes = text.as_bytes();
        let mut dec = Utf8Stream { carry: Vec::new() };
        let mut got = String::new();
        for chunk in bytes.chunks(3) {
            dec.push(chunk, &mut |s| got.push_str(s));
        }
        assert_eq!(got, text);
    }

    #[test]
    fn scrub_patterns() {
        let names = vec!["examplehost".to_string(), "jdoe".to_string()];
        let k = |s: &str| scrub_line(s, &names);
        assert_eq!(k("p50 = 3.2 ms over 1000-2000 samples"), Vec::<&str>::new());
        assert!(k(r"raw data in C:\Users\jdoe\AppData\x.json").contains(&"an absolute user path"));
        assert!(k("path /c/Users/somebody/x").contains(&"an absolute user path"));
        assert!(k("stored in /home/alice/data").contains(&"an absolute user path"));
        assert!(k(r"C:\Users\<user>\AppData").is_empty());
        assert!(k("ran on EXAMPLEHOST at night").contains(&"a user or host name"));
        assert!(k("Volume Serial Number is 1A2B-3C4D").contains(&"a volume serial"));
        assert!(k("guid {3F2504E0-4F89-11D3-9A0C-0305E82C3301}").contains(&"a machine GUID"));
        assert!(k(r#""C:\tools\probe.exe" --window 3"#).contains(&"a process command line"));
        assert!(k("CommandLine: something").contains(&"a process command line"));
        assert!(k("blake3 of the fixture: 64 hex digits are fine").is_empty());
    }

    #[test]
    fn boot_ids_need_a_value() {
        let k = |s: &str| scrub_line(s, &[]);
        for bad in [
            "boot_id = 12345",
            "BootId: 4711",
            "BootId is 0x1F2E",
            "boot id \"8c1b7a4e-51d2-4c79-9a4e-5f1e2d3c4b5a\"",
            "kern.bootsessionuuid boot-id {8C1B7A4E-51D2-4C79-9A4E-5F1E2D3C4B5A}",
            "boot hash 0123456789abcdef0123456789abcdef",
        ] {
            assert!(k(bad).contains(&"a BootId"), "{bad}");
        }
        for ok in [
            "BootId unchanged in 5 of 5 reboots",
            "the boot identity was read 3 times; BootId changed after 2 of 2 reboots",
            "id 0123456789abcdef0123456789abcdef",
            "fixture digest 0123456789abcdef0123456789abcdef",
            "reboots: 5; bootids distinct: yes",
        ] {
            assert!(!k(ok).contains(&"a BootId"), "{ok}");
        }
    }

    #[test]
    fn whole_words_with_non_ascii_names() {
        // A Cyrillic user name after an ASCII letter used to split a character and panic.
        let names = vec!["Иван".to_string(), "ПК-Иванова".to_string()];
        assert!(scrub_line("aИван был здесь", &names).is_empty());
        assert!(scrub_line("xИванов", &names).is_empty());
        assert!(scrub_line("запуск: Иван, 3 ms", &names).contains(&"a user or host name"));
        assert!(scrub_line("host ПК-Иванова", &names).contains(&"a user or host name"));
        assert!(!has_word_ci("éИван", "иван"));
        assert!(has_word_ci("(ИВАН)", "иван"));
        assert!(!has_word_ci("", "x"));
    }

    #[test]
    fn unified_diff_blocks() {
        let diff = "diff --git a/x.txt b/x.txt\nindex 1..2 100644\n--- a/x.txt\n+++ b/x.txt\n@@ -1,0 +1,2 @@\n+one\n+two\n@@ -5 +7 @@\n-old\n+new\ndiff --git a/\"q\\tp\" b/\"q\\tp\"\n--- /dev/null\n+++ \"b/q\\tp\"\n@@ -0,0 +1 @@\n+tab\n";
        let mut got = Vec::new();
        each_added_line(diff.as_bytes(), &mut |p, l, t, s| {
            got.push((p.to_string(), l, t.to_string(), s))
        })
        .unwrap();
        assert_eq!(
            got,
            vec![
                ("x.txt".into(), 1, "one".into(), true),
                ("x.txt".into(), 2, "two".into(), false),
                ("x.txt".into(), 7, "new".into(), true),
                ("q\tp".into(), 1, "tab".into(), true),
            ]
        );
        let raw = b":000000 100644 0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 A\0a b.txt\0";
        let c = parse_raw_z(raw);
        assert_eq!(c[0].path, "a b.txt");
        assert_eq!(c[0].blob, "1111111111111111111111111111111111111111");
        assert_eq!((c[0].mode.as_str(), c[0].status.as_str()), ("100644", "A"));
        assert!(is_report_path("docs/measurements/m0/11.md"));
        assert!(is_report_path("docs/m0/replay-report.md"));
        assert!(!is_report_path("docs/m0/PLAN.md"));
    }

    #[test]
    fn combined_diffs_of_merges() {
        // `git diff-tree --cc` of a merge that adds an evil line and a new file (recorded from git 2.54).
        let diff = "diff --cc f.txt\nindex 9d8d722,2b1936b..f103ee3\n--- a/f.txt\n+++ b/f.txt\n@@@ -3,0 -2,0 +3,1 @@@\n++EVIL LINE here\n@@@ -5,1 -5,1 +6,3 @@@\n  kept\n+ from the first parent's side\n- dropped\n++second evil\ndiff --cc n.txt\nindex 0000000,0000000..a9963be\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@@ -1,0 -1,0 +1,1 @@@\n++new file in merge\n";
        let mut got = Vec::new();
        each_added_line(diff.as_bytes(), &mut |p, l, t, s| {
            got.push((p.to_string(), l, t.to_string(), s))
        })
        .unwrap();
        assert_eq!(
            got,
            vec![
                ("f.txt".into(), 3, "EVIL LINE here".into(), true),
                ("f.txt".into(), 8, "second evil".into(), true),
                ("n.txt".into(), 1, "new file in merge".into(), true),
            ]
        );
        let raw = b"::100644 100644 100644 9d8d722f72ae9fb1410e71a1283496773b4b6fb3 2b1936bb6e1c921cfba26c27a85f60ccf52311a8 f103ee33faca89ccf443cc2d25db3d43d98ce576 MM\0f.txt\0::000000 000000 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 a9963be4af8e4c7b9f1e7d913234b7553cfded97 AA\0n.txt\0";
        let c = parse_raw_z(raw);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].blob, "f103ee33faca89ccf443cc2d25db3d43d98ce576");
        assert_eq!((c[1].path.as_str(), c[1].status.as_str()), ("n.txt", "AA"));
        assert_eq!(c[1].mode, "100644");
    }

    #[test]
    fn unrecognised_paths_fail_closed() {
        for diff in [
            // diff.noprefix
            "diff --git x.txt x.txt\n--- x.txt\n+++ x.txt\n@@ -0,0 +1 @@\n+pasted\n",
            // diff.mnemonicPrefix
            "diff --git c/x.txt i/x.txt\n--- c/x.txt\n+++ i/x.txt\n@@ -0,0 +1 @@\n+pasted\n",
            // a hunk with no header at all
            "diff --git a/x b/x\n@@ -0,0 +1 @@\n+pasted\n",
        ] {
            let r = each_added_line(diff.as_bytes(), &mut |_, _, _, _| {});
            assert!(r.is_err(), "{diff}");
        }
        // A deletion's `+++ /dev/null` with only removed lines is fine.
        let del = "diff --git a/x b/x\ndeleted file mode 100644\n--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n";
        assert!(each_added_line(del.as_bytes(), &mut |_, _, _, _| {}).is_ok());
    }

    #[test]
    fn lf_normalisation_follows_git() {
        let norm = |parts: &[&[u8]]| {
            let mut n = LfNorm::default();
            for p in parts {
                n.update(p);
            }
            n.finish()
        };
        let b3 = |b: &[u8]| *blake3::hash(b).as_bytes();
        let lf = b"one\ntwo\n";
        assert_eq!(norm(&[b"one\r\ntwo\r\n"]), Some(b3(lf)));
        // A CR at a chunk boundary.
        assert_eq!(norm(&[b"one\r", b"\ntwo\r", b"\n"]), Some(b3(lf)));
        // Mixed line ends are normalised too.
        assert_eq!(norm(&[b"one\ntwo\r\n"]), Some(b3(lf)));
        // Git stores these unchanged: no CRLF, a lone CR, a NUL, too many control bytes.
        assert_eq!(norm(&[b"one\ntwo\n"]), None);
        assert_eq!(norm(&[b"one\rtwo\r\n"]), None);
        assert_eq!(norm(&[b"one\r\n\0"]), None);
        assert_eq!(norm(&[b"\x01\x02\r\n"]), None);
        assert_eq!(norm(&[b"a\r"]), None);
        // A trailing ^Z is not counted as a control byte.
        let mut text = vec![b'x'; 128];
        text.extend_from_slice(b"\r\n\x1a");
        assert!(norm(&[&text]).is_some());
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moirai-xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn manifest_round_trip_staleness_and_checks() {
        let d = scratch("manifest");
        std::fs::create_dir_all(d.join("corpora")).unwrap();
        std::fs::write(
            d.join("corpora/notes.txt"),
            "alpha beta gamma delta epsilon zeta eta theta iota kappa\r\nsecond line here\r\n",
        )
        .unwrap();
        std::fs::write(d.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        let public = shingles_of("alpha beta gamma delta epsilon zeta eta theta");
        let st = index(&d, &public).unwrap();
        assert_eq!(st.files, 2);
        assert_eq!(st.public_removed, 1);
        let mut all = Vec::new();
        let m = read_manifest(&d.join(MANIFEST), &mut |h| all.push(h)).unwrap();
        assert_eq!(m.files.len(), 2);
        assert_eq!(all.len() as u64, m.shingles);
        assert!(!all.contains(&public[0]));
        let notes = m
            .files
            .iter()
            .find(|f| f.path == "corpora/notes.txt")
            .unwrap();
        assert_eq!(
            notes.lf,
            Some(
                *blake3::hash(
                    b"alpha beta gamma delta epsilon zeta eta theta iota kappa\nsecond line here\n"
                )
                .as_bytes()
            )
        );
        assert!(
            m.files
                .iter()
                .any(|f| f.path == "blob.bin" && f.lf.is_none())
        );
        assert!(load_current(&d).unwrap().is_some());
        // A changed file makes the manifest stale.
        std::fs::write(d.join("blob.bin"), [9u8]).unwrap();
        assert!(load_current(&d).unwrap_err().contains("stale"));
        index(&d, &[]).unwrap();
        let (m, mp) = load_current(&d).unwrap().unwrap();
        let names: Vec<String> = Vec::new();
        let g = Guards {
            manifest: Some((&d, &m, mp.clone())),
            names: &names,
        };
        // A pasted private line (a partial copy), a copied private file (as git stores it: CRLF normalised) and a
        // private/ path are refused.
        let diff = "diff --git a/docs/a.md b/docs/a.md\n--- a/docs/a.md\n+++ b/docs/a.md\n@@ -0,0 +1,2 @@\n+Some text: Beta Gamma delta epsilon zeta eta theta iota kappa.\n+unrelated words that are public anyway and many more of them\n";
        let lf_blob = m
            .files
            .iter()
            .find(|f| f.path == "corpora/notes.txt")
            .unwrap()
            .lf
            .unwrap();
        let changed = vec![Changed {
            path: "private/x".into(),
            blob: "1".repeat(40),
            mode: "100644".into(),
            status: "A".into(),
        }];
        let mut hits = Hits::default();
        let mut dg = check_changes(
            "staged",
            &changed,
            &[("copied.txt".into(), lf_blob)],
            diff.as_bytes(),
            &g,
            &mut hits,
        )
        .unwrap();
        hits.resolve(&mp, &mut dg).unwrap();
        let msgs: Vec<String> = dg.iter().map(|x| x.to_string()).collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("private/x") && m.contains("never committed")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("copied.txt") && m.contains("BLAKE3 is listed")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("docs/a.md:1") && m.contains("partial copy")),
            "{msgs:#?}"
        );
        assert!(!msgs.iter().any(|m| m.contains("docs/a.md:2")), "{msgs:#?}");
        // A damaged manifest fails closed, even when nothing was added.
        let mut bytes = std::fs::read(d.join(MANIFEST)).unwrap();
        let n = bytes.len();
        bytes[n - 5] ^= 1;
        std::fs::write(d.join(MANIFEST), bytes).unwrap();
        assert!(read_manifest(&d.join(MANIFEST), &mut |_| {}).is_err());
        assert!(Hits::default().resolve(&mp, &mut Vec::new()).is_err());
        // A manifest of another version asks for a re-index.
        std::fs::write(d.join(MANIFEST), "moirai private manifest v1\n").unwrap();
        assert!(read_header(&mp).unwrap_err().contains("re-run"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn index_spills_and_merges_runs() {
        let d = scratch("runs");
        // Enough words for many spills with a run cap of 8 hashes, with repeats across files.
        let text: String = (0..300).map(|i| format!("w{} ", i % 97)).collect();
        std::fs::write(d.join("a.txt"), &text).unwrap();
        std::fs::write(d.join("b.txt"), &text).unwrap();
        let other: String = (0..120).map(|i| format!("v{i} ")).collect();
        std::fs::write(d.join("c.txt"), &other).unwrap();
        let mut expect: Vec<u64> = shingles_of(&text);
        expect.extend(shingles_of(&other));
        expect.sort_unstable();
        expect.dedup();
        let public = vec![expect[3], expect[10], u64::MAX];
        let st = index_with(&d, &public, 8).unwrap();
        assert!(st.runs > 3, "{} runs", st.runs);
        assert_eq!(st.public_removed, 2);
        let mut got = Vec::new();
        let m = read_manifest(&d.join(MANIFEST), &mut |h| got.push(h)).unwrap();
        expect.retain(|h| !public.contains(h));
        assert_eq!(got, expect);
        assert_eq!(m.shingles, expect.len() as u64);
        // The run files are gone, and the manifest never indexes its own temporary files.
        let left: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(MANIFEST) && n != MANIFEST)
            .collect();
        assert!(left.is_empty(), "{left:?}");
        std::fs::write(d.join(format!("{MANIFEST}.run7.tmp")), b"x").unwrap();
        assert!(load_current(&d).unwrap().is_some());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn collector_sorts_amortised() {
        let mut c = Collector {
            v: Vec::new(),
            next_sort: 4,
        };
        for h in [5u64, 1, 5, 3, 1, 9, 3, 3, 7, 1] {
            c.push(h);
        }
        assert_eq!(c.finish(), vec![1, 3, 5, 7, 9]);
    }
}
