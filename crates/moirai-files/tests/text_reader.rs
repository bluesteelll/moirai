//! WP-62: the two-pass streaming reader ([F20 §2.1–§2.5], [40 §2.5]) against the whole-buffer definitions, over
//! sources that cut the content at arbitrary points; stability and retries; the reader's heap on a 16 MiB file.
//!
//! Every source here is synthetic and in memory: a product crate's tests open no file ([OS/README §2.5], PLAN R18).

use moirai_files::oid::{ObjectFormat, Oid, blob_oid};
use moirai_files::text::{
    BUF_SIZE, ByteSource, Content, ContentReader, LineSink, ReadError, ReadOptions, Snapshot,
    Unavailable, analyse,
};
use proptest::prelude::*;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use xxhash_rust::xxh3::xxh3_64;

// --- the whole-buffer definitions, written from [F20 §2.1–§2.5] alone -----------------------------------------

struct NaiveStats {
    crlf: u64,
    lonecr: u64,
    nul: u64,
    printable: u64,
    nonprintable: u64,
}

fn naive_stats(b: &[u8]) -> NaiveStats {
    let (mut crlf, mut lonecr, mut nul, mut printable, mut nonprintable) = (0, 0, 0, 0u64, 0u64);
    let n = b.len();
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == 0x0D {
            if i + 1 < n && b[i + 1] == 0x0A {
                crlf += 1;
                i += 2;
                continue;
            }
            lonecr += 1;
            i += 1;
            continue;
        }
        if c == 0x0A {
            i += 1;
            continue;
        }
        if c == 0x7F {
            nonprintable += 1;
        } else if c < 0x20 {
            if [0x08, 0x09, 0x0C, 0x1B].contains(&c) {
                printable += 1;
            } else {
                if c == 0x00 {
                    nul += 1;
                }
                nonprintable += 1;
            }
        } else {
            printable += 1;
        }
        i += 1;
    }
    if n >= 1 && b[n - 1] == 0x1A {
        nonprintable -= 1;
    }
    NaiveStats {
        crlf,
        lonecr,
        nul,
        printable,
        nonprintable,
    }
}

fn naive_is_text(b: &[u8]) -> bool {
    let s = naive_stats(b);
    s.lonecr == 0 && s.nul == 0 && (s.printable >> 7) >= s.nonprintable
}

fn naive_norm(b: &[u8]) -> Vec<u8> {
    if !naive_is_text(b) {
        return b.to_vec();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x0D && i + 1 < b.len() && b[i + 1] == 0x0A {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn naive_oid(format: ObjectFormat, b: &[u8]) -> String {
    let n = naive_norm(b);
    let mut input = format!("blob {}\0", n.len()).into_bytes();
    input.extend_from_slice(&n);
    let digest: Vec<u8> = match format {
        ObjectFormat::Sha1 => Sha1::digest(&input).to_vec(),
        ObjectFormat::Sha256 => Sha256::digest(&input).to_vec(),
    };
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn naive_lines(t: &[u8]) -> Vec<Vec<u8>> {
    let mut pieces: Vec<Vec<u8>> = t.split(|&b| b == 0x0A).map(<[u8]>::to_vec).collect();
    if pieces.last().is_some_and(Vec::is_empty) {
        pieces.pop();
    }
    pieces
}

fn naive_atext(b: &[u8]) -> Vec<u8> {
    let n = naive_norm(b);
    n.strip_prefix(&[0xEF, 0xBB, 0xBF][..])
        .unwrap_or(&n)
        .to_vec()
}

fn naive_nl(l: &[u8]) -> &[u8] {
    let ws = [0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20];
    let mut l = l;
    while let [first, rest @ ..] = l {
        if !ws.contains(first) {
            break;
        }
        l = rest;
    }
    while let [rest @ .., last] = l {
        if !ws.contains(last) {
            break;
        }
        l = rest;
    }
    l
}

fn naive_trivial(l: &[u8]) -> bool {
    naive_nl(l)
        .iter()
        .all(|b| b"\t\n\x0b\x0c\r {}()[];,".contains(b))
}

/// `wh(l) = low(XXH3-64(nl(l)), 16)`: the first two bytes of the stored hash, read little-endian ([F20 §1.2, §2.7.1]).
fn naive_wh(l: &[u8]) -> u16 {
    let h = xxh3_64(naive_nl(l)).to_le_bytes();
    u16::from_le_bytes([h[0], h[1]])
}

// --- sources and sinks ---------------------------------------------------------------------------------------

/// Content in memory, read in pieces whose sizes cycle through `sizes`.
struct Chunky<'a> {
    data: &'a [u8],
    pos: usize,
    sizes: Vec<usize>,
    k: usize,
}

impl<'a> Chunky<'a> {
    fn new(data: &'a [u8], sizes: &[usize]) -> Chunky<'a> {
        Chunky {
            data,
            pos: 0,
            sizes: sizes.to_vec(),
            k: 0,
        }
    }
}

impl ByteSource for Chunky<'_> {
    type Error = std::convert::Infallible;
    type Stamp = u64;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let want = self.sizes[self.k % self.sizes.len()];
        self.k += 1;
        let n = want.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot<u64>, Self::Error> {
        Ok(Snapshot {
            size: self.data.len() as u64,
            mtime: 1,
        })
    }
}

/// Collects the lines the reader hands to the pass-1 hook.
#[derive(Default)]
struct Collect {
    lines: Vec<Vec<u8>>,
    cur: Vec<u8>,
    begins: u32,
}

impl LineSink for Collect {
    fn begin(&mut self) {
        self.lines.clear();
        self.cur.clear();
        self.begins += 1;
    }
    fn piece(&mut self, bytes: &[u8]) {
        assert!(!bytes.is_empty());
        self.cur.extend_from_slice(bytes);
    }
    fn end_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.cur));
    }
}

fn opts(format: ObjectFormat, max_line_hashes: Option<u32>) -> ReadOptions {
    ReadOptions {
        format,
        max_read_bytes: 16 << 20,
        max_line_hashes,
    }
}

fn read_chunky(
    data: &[u8],
    sizes: &[usize],
    format: ObjectFormat,
    cap: Option<u32>,
) -> (Content, Collect) {
    let mut sink = Collect::default();
    let mut src = Chunky::new(data, sizes);
    let c = ContentReader::new()
        .read(&mut src, &opts(format, cap), &mut sink)
        .expect("stable read");
    (c, sink)
}

/// Checks every result of one read against the whole-buffer definitions.
fn check(data: &[u8], sizes: &[usize], cap: u32) {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        let (c, sink) = read_chunky(data, sizes, format, Some(cap));
        let text = naive_is_text(data);
        let s = naive_stats(data);
        assert_eq!(c.is_text(), text);
        assert_eq!(
            (
                c.stats.crlf,
                c.stats.lonecr,
                c.stats.nul,
                c.stats.printable,
                c.stats.nonprintable
            ),
            (s.crlf, s.lonecr, s.nul, s.printable, s.nonprintable)
        );
        assert_eq!(c.raw_len(), data.len() as u64);
        assert_eq!(c.norm_len(), naive_norm(data).len() as u64);
        assert_eq!(c.nlines(), naive_lines(&naive_norm(data)).len() as u64);
        assert_eq!(c.raw_xxh3, xxh3_64(data));
        assert_eq!(c.oid.to_string(), naive_oid(format, data));
        assert_eq!(c.oid.algo(), format.algo());
        // In memory, one chunk: the same.
        let m = analyse(data, format, Some(cap), &mut ());
        assert_eq!((m.oid, m.stats, m.raw_xxh3), (c.oid, c.stats, c.raw_xxh3));
        if text {
            let t = naive_atext(data);
            let lines = naive_lines(&t);
            assert_eq!(sink.lines, lines);
            let h = c.line_hashes.as_ref().expect("line hashes of text");
            assert_eq!(h.total_lines(), lines.len() as u64);
            assert_eq!(h.is_complete(), lines.len() <= cap as usize);
            // `wh(l)` for a non-trivial line, nothing for a trivial one ([F20 §2.7.1]).
            let want: Vec<Option<u16>> = lines
                .iter()
                .take(cap as usize)
                .map(|l| (!naive_trivial(l)).then(|| naive_wh(l)))
                .collect();
            let got: Vec<Option<u16>> = h.iter().map(|x| x.window_hash()).collect();
            assert_eq!(got, want);
            assert!(
                h.iter()
                    .all(|x| x.is_trivial() == x.window_hash().is_none())
            );
            assert_eq!(
                m.line_hashes.as_ref().map(|x| x.iter().collect::<Vec<_>>()),
                Some(h.iter().collect())
            );
        } else {
            assert!(c.line_hashes.is_none());
        }
    }
}

// --- the property tests ------------------------------------------------------------------------------------------

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed), so a
/// failing case never writes a regressions file into the repository.
fn test_config(base: u32) -> ProptestConfig {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => base * 16,
        Ok("exit") => base * 64,
        _ => base,
    };
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Bytes that exercise every rule: CR, LF, NUL, `^Z`, DEL, other controls, the BOM bytes, whitespace, brackets.
fn byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        6 => prop::sample::select(b"ab xyz{};,()".to_vec()),
        3 => Just(0x0D),
        3 => Just(0x0A),
        2 => prop::sample::select(vec![0x09, 0x0B, 0x0C, 0x20]),
        2 => prop::sample::select(vec![0xEF, 0xBB, 0xBF, 0xC3, 0xA9]),
        1 => prop::sample::select(vec![0x00, 0x1A, 0x7F, 0x01, 0x08, 0x1B]),
    ]
}

/// Mostly text: lines of printable bytes, CRLF or LF ends, an optional BOM, rare controls near the 1/128 ratio.
fn texty() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<bool>(),
        prop::collection::vec(("[ -~]{0,40}", any::<bool>()), 0..40),
        prop::collection::vec(prop::sample::select(vec![0x01u8, 0x1A, 0x7F, 0x0D]), 0..3),
    )
        .prop_map(|(bom, lines, controls)| {
            let mut v = Vec::new();
            if bom {
                v.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            for (l, crlf) in lines {
                v.extend_from_slice(l.as_bytes());
                v.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
            }
            for (i, c) in controls.into_iter().enumerate() {
                let at = (i * 97) % (v.len() + 1);
                v.insert(at, c);
            }
            v
        })
}

proptest! {
    #![proptest_config(test_config(512))]

    /// The `oid` of the normalised stream, and every other pass-1 result, equals the whole-buffer computation for
    /// every way of cutting the content ([F20 §2.4] "Buffers never change a result").
    #[test]
    fn streaming_equals_whole_buffer(
        data in prop::collection::vec(byte(), 0..300),
        sizes in prop::collection::vec(1usize..=11, 1..6),
        cap in 0u32..40,
    ) {
        check(&data, &sizes, cap);
    }

    #[test]
    fn streaming_equals_whole_buffer_on_text(
        data in texty(),
        sizes in prop::collection::vec(1usize..=64, 1..6),
        cap in 0u32..60,
    ) {
        check(&data, &sizes, cap);
    }
}

// --- the cases WP-62 names ---------------------------------------------------------------------------------------

fn sha1_hex(data: &[u8], sizes: &[usize]) -> (String, Content) {
    let (c, _) = read_chunky(data, sizes, ObjectFormat::Sha1, Some(1024));
    (c.oid.to_string(), c)
}

#[test]
fn crlf_is_normalised() {
    let (oid, c) = sha1_hex(b"hello\r\n", &[3]);
    assert_eq!(oid, "ce013625030ba8dba906f756967f9e9ca394464a");
    assert!(c.is_text());
    assert_eq!((c.raw_len(), c.norm_len(), c.nlines()), (7, 6, 1));
    // The CR at the end of a read is classified with the next read's first byte.
    for s in 1..=4 {
        assert_eq!(
            sha1_hex(b"a\r\nb", &[s]).0,
            "0a207c060e61f3b88eaee0a8cd0696f46fb155eb"
        );
    }
    // Mixed endings: every pair normalised, lone LFs kept.
    assert_eq!(
        sha1_hex(b"a\r\nb\nc\r\n", &[2]).0,
        blob_oid(ObjectFormat::Sha1, b"a\nb\nc\n").to_string()
    );
}

#[test]
fn lone_cr_makes_binary() {
    for s in 1..=3 {
        let (oid, c) = sha1_hex(b"a\rb", &[s]);
        assert_eq!(oid, "2fe40ba389048204a83882bc3f75bf2188db6d47");
        assert!(!c.is_text());
        assert!(c.line_hashes.is_none());
    }
    // A final CR is lone too, and a CR LF elsewhere is then kept.
    let (oid, c) = sha1_hex(b"a\r\nb\r", &[4]);
    assert!(!c.is_text());
    assert_eq!(oid, blob_oid(ObjectFormat::Sha1, b"a\r\nb\r").to_string());
}

#[test]
fn nul_makes_binary() {
    let data = b"a\x00b\r\nc";
    let (oid, c) = sha1_hex(data, &[1]);
    assert!(!c.is_text());
    assert_eq!(c.norm_len(), 6);
    assert_eq!(oid, blob_oid(ObjectFormat::Sha1, data).to_string());
    // Too many non-printable bytes: binary by ratio, with no NUL.
    let mut data = vec![b'A'; 127];
    data.push(0x01);
    assert!(!sha1_hex(&data, &[5]).1.is_text());
    data.insert(0, b'A');
    assert!(sha1_hex(&data, &[5]).1.is_text());
}

#[test]
fn final_ctrl_z_is_not_counted() {
    let (oid, c) = sha1_hex(b"x\r\n\x1a", &[1]);
    assert_eq!(oid, "2f484e3cd37e421218bc8a9929df89ceb50dbb1a");
    assert!(c.is_text());
    assert_eq!(c.stats.nonprintable, 0);
    // A `^Z` that is not last counts.
    let mut data = vec![b'A'; 127];
    data.extend_from_slice(b"\x1a\n");
    assert!(!sha1_hex(&data, &[9]).1.is_text());
}

#[test]
fn bom_is_kept_in_oid_and_removed_from_lines() {
    let data = b"\xef\xbb\xbf  first  \r\nsecond\r\n";
    for s in 1..=5 {
        let (c, sink) = read_chunky(data, &[s], ObjectFormat::Sha1, Some(8));
        assert!(c.is_text());
        assert_eq!(
            c.oid,
            blob_oid(ObjectFormat::Sha1, b"\xef\xbb\xbf  first  \nsecond\n")
        );
        assert_eq!(sink.lines, vec![b"  first  ".to_vec(), b"second".to_vec()]);
        let h = c.line_hashes.unwrap();
        assert_eq!(h.get(0).unwrap().window_hash(), Some(naive_wh(b"first")));
    }
    // A BOM prefix that is not a BOM stays in the first line.
    let (_, sink) = read_chunky(b"\xef\xbbx\n", &[1], ObjectFormat::Sha1, None);
    assert_eq!(sink.lines, vec![b"\xef\xbbx".to_vec()]);
    // Only a BOM: no line of anchor text, one line of `norm(b)` ([F20 §2.5] vs §2.6.3).
    let (c, sink) = read_chunky(b"\xef\xbb\xbf", &[2], ObjectFormat::Sha1, Some(4));
    assert!(sink.lines.is_empty());
    assert_eq!(c.nlines(), 1);
    assert_eq!(c.line_hashes.unwrap().total_lines(), 0);
}

#[test]
fn empty_content() {
    let (oid, c) = sha1_hex(b"", &[1]);
    assert_eq!(oid, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert!(c.is_text());
    assert_eq!((c.raw_len(), c.norm_len(), c.nlines()), (0, 0, 0));
    assert_eq!(c.line_hashes.unwrap().total_lines(), 0);
    let (c, _) = read_chunky(b"", &[1], ObjectFormat::Sha256, None);
    assert_eq!(c.oid, blob_oid(ObjectFormat::Sha256, b""));
    assert_eq!(c.raw_xxh3, xxh3_64(b""));
}

// --- stability, retries and failures ([F20 §2.4], review A1P-05) -------------------------------------------

/// A file an in-place writer changes: `versions[i]` is served from the `switch_at[i]`-th read call on, with a
/// distinct last-write time per version unless `same_stamp`.
struct Racing {
    versions: Vec<Vec<u8>>,
    switch_at: Vec<u32>,
    reads: u32,
    pos: usize,
    same_stamp: bool,
}

impl Racing {
    fn current(&self) -> usize {
        self.switch_at.iter().filter(|&&s| self.reads >= s).count()
    }
}

impl ByteSource for Racing {
    type Error = std::convert::Infallible;
    type Stamp = usize;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let v = &self.versions[self.current()];
        self.reads += 1;
        let n = buf.len().min(3).min(v.len().saturating_sub(self.pos));
        buf[..n].copy_from_slice(&v[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<usize>, Self::Error> {
        let v = self.current();
        Ok(Snapshot {
            size: self.versions[v].len() as u64,
            mtime: if self.same_stamp { 0 } else { v },
        })
    }
}

#[test]
fn a_change_between_passes_is_retried_once() {
    // Changed during pass 2 of the first attempt (same size, same time stamp: only r1 ≠ r2 shows it); the retry reads
    // the new content.
    let old = b"alpha\r\nbeta\r\n".to_vec();
    let new = b"alpha\r\nBETA\r\n".to_vec();
    let mut src = Racing {
        versions: vec![old, new.clone()],
        switch_at: vec![7],
        reads: 0,
        pos: 0,
        same_stamp: true,
    };
    let mut sink = Collect::default();
    let c = ContentReader::new()
        .read(&mut src, &opts(ObjectFormat::Sha1, Some(8)), &mut sink)
        .unwrap();
    assert_eq!(c.oid, blob_oid(ObjectFormat::Sha1, b"alpha\nBETA\n"));
    assert_eq!(sink.begins, 2);
    assert_eq!(sink.lines, vec![b"alpha".to_vec(), b"BETA".to_vec()]);

    // A growing file: pass 1 reads past the size the handle reported.
    let mut src = Racing {
        versions: vec![b"abc".to_vec(), b"abcdef".to_vec(), b"abcdef".to_vec()],
        switch_at: vec![1, 1_000],
        reads: 0,
        pos: 0,
        same_stamp: false,
    };
    let c = ContentReader::new()
        .read(&mut src, &opts(ObjectFormat::Sha1, None), &mut ())
        .unwrap();
    assert_eq!(c.oid, blob_oid(ObjectFormat::Sha1, b"abcdef"));
}

#[test]
fn two_unstable_reads_are_unavailable() {
    let versions: Vec<Vec<u8>> = (0..10u8).map(|i| vec![b'a' + i; 9]).collect();
    let mut src = Racing {
        versions,
        switch_at: (1..10).map(|i| i * 2).collect(),
        reads: 0,
        pos: 0,
        same_stamp: true,
    };
    let err = ContentReader::new()
        .read(&mut src, &opts(ObjectFormat::Sha1, None), &mut ())
        .unwrap_err();
    assert!(matches!(err, ReadError::Unstable));
    assert_eq!(err.reason(), Unavailable::Unstable);
    assert_eq!(err.reason().as_str(), "unstable");
}

#[test]
fn a_changed_last_write_time_alone_is_retried() {
    // The same bytes and size in every version: only the last-write time shows the in-place write. Each pass of an
    // attempt makes 5 read calls here (3, 3, 3 and 2 bytes, then the end), so attempt 1 makes calls 0–9.
    let data = b"same bytes\n".to_vec();
    let mut src = Racing {
        versions: vec![data.clone(), data.clone()],
        switch_at: vec![3],
        reads: 0,
        pos: 0,
        same_stamp: false,
    };
    let mut sink = Collect::default();
    let c = ContentReader::new()
        .read(&mut src, &opts(ObjectFormat::Sha1, Some(8)), &mut sink)
        .unwrap();
    assert_eq!(sink.begins, 2);
    assert_eq!(c.oid, blob_oid(ObjectFormat::Sha1, &data));
    assert_eq!(sink.lines, vec![b"same bytes".to_vec()]);

    // A last-write time that changes during every attempt: unstable after the one retry.
    let mut src = Racing {
        versions: vec![data; 10],
        switch_at: (1..10).map(|i| i * 3).collect(),
        reads: 0,
        pos: 0,
        same_stamp: false,
    };
    let mut sink = Collect::default();
    let err = ContentReader::new()
        .read(&mut src, &opts(ObjectFormat::Sha1, None), &mut sink)
        .unwrap_err();
    assert!(matches!(err, ReadError::Unstable));
    assert_eq!(sink.begins, 2);
}

#[test]
fn a_source_not_at_offset_zero_is_read_from_the_start() {
    // A reused handle, or one another reader moved: every attempt rewinds first, so no retry is spent on it.
    let data = b"alpha\r\n  beta\r\n";
    for start in [1, 7, data.len()] {
        let mut src = Chunky::new(data, &[4]);
        src.pos = start;
        let mut sink = Collect::default();
        let c = ContentReader::new()
            .read(&mut src, &opts(ObjectFormat::Sha1, Some(8)), &mut sink)
            .unwrap();
        assert_eq!(sink.begins, 1, "one attempt from offset {start}");
        assert_eq!(c.oid, blob_oid(ObjectFormat::Sha1, b"alpha\n  beta\n"));
        assert_eq!(sink.lines, vec![b"alpha".to_vec(), b"  beta".to_vec()]);
    }
}

/// A source whose n-th rewind or n-th snapshot (1-based) fails.
struct Failing {
    data: &'static [u8],
    pos: usize,
    rewinds: u32,
    snapshots: std::cell::Cell<u32>,
    fail_rewind: u32,
    fail_snapshot: u32,
}

impl Failing {
    fn new(fail_rewind: u32, fail_snapshot: u32) -> Failing {
        Failing {
            data: b"one\r\ntwo\r\n",
            pos: 0,
            rewinds: 0,
            snapshots: std::cell::Cell::new(0),
            fail_rewind,
            fail_snapshot,
        }
    }
}

impl ByteSource for Failing {
    type Error = &'static str;
    type Stamp = ();
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn rewind(&mut self) -> Result<(), &'static str> {
        self.rewinds += 1;
        if self.rewinds == self.fail_rewind {
            return Err("seek failed");
        }
        self.pos = 0;
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<()>, &'static str> {
        self.snapshots.set(self.snapshots.get() + 1);
        if self.snapshots.get() == self.fail_snapshot {
            return Err("stat failed");
        }
        Ok(Snapshot {
            size: self.data.len() as u64,
            mtime: (),
        })
    }
}

#[test]
fn failing_rewinds_and_snapshots_are_unreadable() {
    let read = |src: &mut Failing| {
        ContentReader::new()
            .read(src, &opts(ObjectFormat::Sha1, Some(4)), &mut ())
            .map(|c| c.oid)
    };
    // One attempt: snapshot 1, rewind 1, pass 1, rewind 2, pass 2, snapshot 2.
    let mut ok = Failing::new(0, 0);
    assert_eq!(
        read(&mut ok).unwrap(),
        blob_oid(ObjectFormat::Sha1, b"one\ntwo\n")
    );
    assert_eq!((ok.rewinds, ok.snapshots.get()), (2, 2));
    for (rewind, snapshot, what) in [
        (1, 0, "seek failed"),
        (2, 0, "seek failed"),
        (0, 1, "stat failed"),
        (0, 2, "stat failed"),
    ] {
        let err = read(&mut Failing::new(rewind, snapshot)).unwrap_err();
        assert!(
            matches!(err, ReadError::Unreadable(e) if e == what),
            "rewind {rewind}, snapshot {snapshot}: {err:?}"
        );
        assert_eq!(err.reason(), Unavailable::Unreadable);
    }
}

#[test]
fn size_limit_and_read_errors() {
    let data = vec![b'x'; 100];
    let mut src = Chunky::new(&data, &[7]);
    let o = ReadOptions {
        format: ObjectFormat::Sha1,
        max_read_bytes: 99,
        max_line_hashes: None,
    };
    let err = ContentReader::new()
        .read(&mut src, &o, &mut ())
        .unwrap_err();
    assert!(matches!(err, ReadError::Size { size: 100 }));
    assert_eq!(err.reason(), Unavailable::Size);
    let o = ReadOptions {
        max_read_bytes: 100,
        ..o
    };
    assert!(ContentReader::new().read(&mut src, &o, &mut ()).is_ok());

    struct Denied;
    impl ByteSource for Denied {
        type Error = &'static str;
        type Stamp = ();
        fn read(&mut self, _: &mut [u8]) -> Result<usize, &'static str> {
            Err("sharing violation")
        }
        fn rewind(&mut self) -> Result<(), &'static str> {
            Ok(())
        }
        fn snapshot(&self) -> Result<Snapshot<()>, &'static str> {
            Ok(Snapshot { size: 4, mtime: () })
        }
    }
    let err = ContentReader::new()
        .read(&mut Denied, &opts(ObjectFormat::Sha1, None), &mut ())
        .unwrap_err();
    assert!(matches!(err, ReadError::Unreadable("sharing violation")));
    assert_eq!(err.reason(), Unavailable::Unreadable);
}

// --- the reader's heap on 16 MiB ([40 §2.5] "Streaming, bounded memory") -------------------------------------

const LINE: &[u8; 64] = b"    let value = compute(alpha, beta, gamma); // synthetic line\r\n";
const SIXTEEN_MIB: u64 = 16 << 20;

/// 16 MiB of CRLF text generated on the fly: no copy of the file exists anywhere.
struct Synthetic {
    pos: u64,
}

impl ByteSource for Synthetic {
    type Error = std::convert::Infallible;
    type Stamp = ();
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let n = usize::try_from((SIXTEEN_MIB - self.pos).min(buf.len() as u64)).unwrap();
        for (i, b) in buf[..n].iter_mut().enumerate() {
            *b = LINE[((self.pos + i as u64) % 64) as usize];
        }
        self.pos += n as u64;
        Ok(n)
    }
    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<()>, Self::Error> {
        Ok(Snapshot {
            size: SIXTEEN_MIB,
            mtime: (),
        })
    }
}

fn synthetic_oid() -> Oid {
    let lines = SIXTEEN_MIB / 64;
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", SIXTEEN_MIB - lines).as_bytes());
    for _ in 0..lines {
        h.update(&LINE[..62]);
        h.update(b"\n");
    }
    Oid::new(ObjectFormat::Sha1.algo(), h.finalize().as_slice()).unwrap()
}

/// The heap budget of one read, PLAN WP-62's 0.5 MB read as 500,000 bytes.
const HEAP_BOUND: usize = 500_000;

/// Heap accounting of a read of a 16 MiB file: the capacities the reader's buffer and the line-hash array report, held
/// under [`HEAP_BOUND`] and above the data they must hold.
///
/// This is a regression guard, not the evidence for PLAN WP-62's "≤ 0.5 MB extra RSS" acceptance item. It adds up
/// self-reported capacities, so it would not notice another allocation on the read path, and it says nothing about
/// RSS. This crate forbids `unsafe` and may not depend on `moirai-os` (PLAN §2.1, §2.2), so it can install no counting
/// allocator and read no RSS; the measurement needs a crate that links both `moirai-files` and `CountingAlloc`/`Meter`,
/// which PLAN §2.2 does not allow yet (a plan finding of the WP-62 review).
#[test]
fn sixteen_mib_heap_accounting() {
    let want = synthetic_oid();
    let mut reader = ContentReader::new();

    // `oid` and statistics only: the fixed buffer.
    let c = reader
        .read(
            &mut Synthetic { pos: 0 },
            &opts(ObjectFormat::Sha1, None),
            &mut (),
        )
        .unwrap();
    assert_eq!(c.oid, want);
    assert_eq!(
        (c.raw_len(), c.norm_len(), c.nlines()),
        (
            SIXTEEN_MIB,
            SIXTEEN_MIB - SIXTEEN_MIB / 64,
            SIXTEEN_MIB / 64
        )
    );
    assert!(c.line_hashes.is_none());
    let heap = reader.heap_bytes() + c.heap_bytes();
    assert!(heap <= HEAP_BOUND, "{heap} bytes");

    // With the line-hash array at the default `files.max-line-hashes` (65,536, [CFG §10.4]): the buffer plus the
    // capped array of 2 bytes and 1 bit per line.
    let cap = 65_536u32;
    let c = reader
        .read(
            &mut Synthetic { pos: 0 },
            &opts(ObjectFormat::Sha1, Some(cap)),
            &mut (),
        )
        .unwrap();
    assert_eq!(c.oid, want);
    let h = c.line_hashes.as_ref().unwrap();
    assert_eq!(
        (h.len(), h.total_lines(), h.is_complete()),
        (cap as usize, SIXTEEN_MIB / 64, false)
    );
    assert_eq!(h.get(0).unwrap().window_hash(), Some(naive_wh(&LINE[..62])));
    let heap = reader.heap_bytes() + c.heap_bytes();
    // At least the data it must hold, so the accounting cannot under-report; at most the budget.
    let floor = BUF_SIZE + 2 * cap as usize + cap as usize / 8;
    assert!(heap >= floor, "{heap} bytes < {floor}");
    assert!(heap <= HEAP_BOUND, "{heap} bytes > {HEAP_BOUND}");
}
