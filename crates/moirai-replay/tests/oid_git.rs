//! WP-74: FL-1's `oid` (WP-62; [F20 §2.1–§2.4]) against `git hash-object`, in a SHA-1 and a SHA-256 scratch
//! repository, on synthetic CRLF, binary, `^Z`, empty and large inputs (WP-62's acceptance) and on generated ones.
//!
//! `oid_H(b) = H("blob " ‖ dec(len(norm(b))) ‖ 00 ‖ norm(b))` equals the blob id git computes where git converts a
//! file exactly as `norm` does ([F20 §2.3]): with `text=auto`, git's `convert_is_binary` over `gather_stats` is
//! [F20 §2.1]'s `is_text`, and a text file's CR LF pairs become LF. So for every input b and both formats:
//!
//! 1. the four product paths agree: `oid_of` (in memory), `analyse` (both passes in memory), and `ContentReader`
//!    (two passes through one 128 KiB buffer, [F20 §2.4]) over the file itself and over a source that answers each
//!    read with an irregular number of bytes, so chunk boundaries fall anywhere and not only where the OS puts them.
//!    The irregular read also records the line-hash array at the default of `files.max-line-hashes` ([CFG]), as a
//!    capture reads ([F20 §2.4]), so the `oid` compared with git is also that configuration's;
//! 2. that `oid` equals `git hash-object` with `text=auto` and with `core.autocrlf=true`;
//! 3. it equals `git hash-object --no-filters` (the raw bytes) exactly when `norm(b) = b`, that is unless b is text
//!    and holds a CR LF pair. That observes git's text decision only on an input with a pair, so every input b is
//!    checked twice: as it is, and as CR LF ‖ b. The prefix changes no count of [F20 §2.1] but `len` and `crlf`
//!    (`stats` is checked to say so) and never the last byte, which the final-`1A` rule reads, so CR LF ‖ b has b's
//!    text decision and a pair: git's own decision (`convert_is_binary`) is observed for every b and must be
//!    `is_text`'s;
//! 4. the raw blob id of [F20 §2.3] "Symbolic links" (`blob_oid`, no `norm`) equals `git hash-object --no-filters`.
//!
//! Each input is built at its final size, with room for the prefix, and is prefixed in place, so a large input is
//! held once ([`crlf_across_buffers`]).
//!
//! The named corpus reaches every buffer edge of [F20 §2.4] ("a CR at the end of a buffer is classified with the next
//! buffer's first byte, and the final-`1A` adjustment uses the last byte of the content"): a CR LF pair and a lone CR
//! straddling a boundary, a lone CR and a `1A` as the last byte of a full buffer at the end of the content, and a `1A`
//! as the last byte of a buffer that is not the last, where the printable ratio decides.
//!
//! `git hash-object` reads no index, so git's "safer autocrlf" rule does not apply to it: `git add` keeps the CR LF
//! pairs of a path whose indexed blob already holds a CR (`convert.c`, `has_crlf_in_index`), and the blob it writes
//! then differs from `oid` — one of the cases [F20 §2.3] "Git evidence never uses `oid`" covers.

mod common;
mod scratch;

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

use moirai_files::oid::{ObjectFormat, Oid, blob_oid};
use moirai_files::text::{
    BUF_SIZE, ByteSource, ContentReader, ReadOptions, Snapshot, TextStats, analyse, oid_of, stats,
};
use moirai_replay::git::{Conversion, Git, GitError, HashObject};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use scratch::Scratch;

/// The default of `files.max-line-hashes` ([CFG]), with which a capture's read records the line-hash array.
const MAX_LINE_HASHES: u32 = 65_536;

/// The prefix of the second check of every input (module documentation, point 3).
const CRLF: &[u8] = b"\r\n";

/// A file as the reader's byte source, as `ProjectFs::read_for_hash` supplies one from M6 ([OS/project §5.5]).
struct FileSource(File);

impl ByteSource for FileSource {
    type Error = io::Error;
    type Stamp = SystemTime;

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn rewind(&mut self) -> io::Result<()> {
        self.0.seek(SeekFrom::Start(0)).map(|_| ())
    }

    fn snapshot(&self) -> io::Result<Snapshot<SystemTime>> {
        let m = self.0.metadata()?;
        Ok(Snapshot {
            size: m.len(),
            mtime: m.modified()?,
        })
    }
}

/// An input in memory as the reader's byte source, answering each read with at most the next of a cycle of sizes,
/// started at an offset, so the reader's chunks end at arbitrary positions ([F20 §2.4] "Buffers never change a
/// result").
struct Irregular<'a> {
    bytes: &'a [u8],
    pos: usize,
    step: usize,
}

impl Irregular<'_> {
    /// Read sizes: single bytes, sizes around 64 (the period of [`crlf_across_buffers`]) and around the buffer size.
    const SIZES: [usize; 11] = [
        1,
        2,
        3,
        63,
        64,
        65,
        1000,
        4097,
        65_537,
        BUF_SIZE - 1,
        BUF_SIZE,
    ];
}

impl ByteSource for Irregular<'_> {
    type Error = std::convert::Infallible;
    type Stamp = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let size = Self::SIZES[self.step % Self::SIZES.len()];
        self.step += 1;
        let n = size.min(buf.len()).min(self.bytes.len() - self.pos);
        buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot<()>, Self::Error> {
        Ok(Snapshot {
            size: self.bytes.len() as u64,
            mtime: (),
        })
    }
}

/// What git and the product decided, over the inputs checked.
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    /// Inputs, each checked as it is and as CR LF ‖ b, in both object formats.
    inputs: u64,
    /// Checks of a text with a CR LF pair: both sides converted.
    converted: u64,
    /// Checks of a text without a CR LF pair.
    text_unchanged: u64,
    /// Checks of binary content.
    binary: u64,
}

/// One scratch repository per object format, with a `hash-object` process per conversion.
struct Differential {
    reader: ContentReader,
    repos: Vec<(ObjectFormat, std::path::PathBuf, Vec<HashObject>)>,
    next: u64,
    seen: Seen,
}

impl Differential {
    fn new(git: &Git, scratch: &Path) -> Differential {
        let mut repos = Vec::new();
        for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
            let dir = scratch.join(format.algo().name());
            let repo = git.init(&dir, format).expect("a scratch repository");
            let hashers = Conversion::ALL
                .iter()
                .map(|&c| repo.hasher(c).expect("git hash-object starts"))
                .collect();
            repos.push((format, dir, hashers));
        }
        Differential {
            reader: ContentReader::new(),
            repos,
            next: 0,
            seen: Seen::default(),
        }
    }

    /// Checks an input b as it is and as CR LF ‖ b (module documentation, point 3), prefixing `bytes` in place; the
    /// error names the check and the step that disagree.
    fn check_both(&mut self, bytes: &mut Vec<u8>) -> Result<(), String> {
        let st = stats(bytes);
        self.check(bytes, &st)
            .map_err(|e| format!("as it is: {e}"))?;
        prepend_crlf(bytes);
        let prefixed = stats(bytes);
        // [F20 §2.1]: the pair adds to `crlf` only, and the last byte stays (an empty b gets the LF as its last).
        let want = TextStats {
            len: st.len + CRLF.len() as u64,
            crlf: st.crlf + 1,
            last: st.last.or(Some(b'\n')),
            ..st
        };
        if prefixed != want {
            return Err(format!(
                "CR LF ‖ b: stats {prefixed:?}, expected {want:?} from b's {st:?}"
            ));
        }
        self.check(bytes, &prefixed)
            .map_err(|e| format!("as CR LF ‖ b: {e}"))?;
        self.seen.inputs += 1;
        Ok(())
    }

    /// Checks one content, whose statistics are `st`, in both repositories; the error names the step that disagrees.
    fn check(&mut self, bytes: &[u8], st: &TextStats) -> Result<(), String> {
        let changes = st.is_text() && st.crlf > 0;
        let case = self.next;
        let name = format!("c{case}.bin");
        self.next += 1;
        for (format, dir, hashers) in &mut self.repos {
            let format = *format;
            let path = dir.join(&name);
            std::fs::write(&path, bytes).map_err(|e| format!("write {name}: {e}"))?;
            let mem = oid_of(format, bytes);
            let both = analyse(bytes, format, None, &mut ()).oid;
            let opts = ReadOptions {
                format,
                max_read_bytes: u64::MAX,
                max_line_hashes: None,
            };
            let file = File::open(&path).map_err(|e| format!("open {name}: {e}"))?;
            let read = self
                .reader
                .read(&mut FileSource(file), &opts, &mut ())
                .map_err(|e| format!("the reader failed: {:?}", e.reason()))?
                .oid;
            let mut irregular = Irregular {
                bytes,
                pos: 0,
                // Each input starts the cycle elsewhere.
                step: usize::try_from(case).unwrap_or(0),
            };
            // As a capture reads: with the line-hash array ([F20 §2.4]).
            let lines = ReadOptions {
                max_line_hashes: Some(MAX_LINE_HASHES),
                ..opts
            };
            let chunked = match self.reader.read(&mut irregular, &lines, &mut ()) {
                Ok(content) if content.line_hashes.is_some() != content.is_text() => {
                    return Err(format!(
                        "the chunked reader recorded line hashes {} for content with is_text {}",
                        content.line_hashes.is_some(),
                        content.is_text()
                    ));
                }
                Ok(content) => content.oid,
                Err(e) => return Err(format!("the chunked reader failed: {:?}", e.reason())),
            };
            if mem != both || mem != read || mem != chunked {
                return Err(format!(
                    "{}: oid_of {mem:?}, analyse {both:?}, ContentReader {read:?}, irregular chunks {chunked:?}",
                    format.algo().name()
                ));
            }
            let raw = blob_oid(format, bytes);
            let mut git: Vec<(Conversion, Oid)> = Vec::new();
            for h in hashers.iter_mut() {
                let oid = h.hash(&name).map_err(|e| e.to_string())?;
                git.push((h.conversion(), oid));
            }
            let _ = std::fs::remove_file(&path);
            for (conversion, oid) in git {
                let want_equal = conversion != Conversion::Raw || !changes;
                if (oid == mem) != want_equal {
                    return Err(format!(
                        "{}: oid {mem:?}, git hash-object ({}) {oid:?}; is_text {}, {} CR LF pairs",
                        format.algo().name(),
                        conversion.name(),
                        st.is_text(),
                        st.crlf
                    ));
                }
                if conversion == Conversion::Raw && oid != raw {
                    return Err(format!(
                        "{}: blob_oid {raw:?}, git hash-object --no-filters {oid:?}",
                        format.algo().name()
                    ));
                }
            }
        }
        if changes {
            self.seen.converted += 1;
        } else if st.is_text() {
            self.seen.text_unchanged += 1;
        } else {
            self.seen.binary += 1;
        }
        Ok(())
    }

    fn finish(self) -> Seen {
        for (_, _, hashers) in self.repos {
            for h in hashers {
                h.finish().expect("git hash-object exits cleanly");
            }
        }
        self.seen
    }
}

fn setup(name: &str) -> (Scratch, Git) {
    let scratch = Scratch::new(name);
    let git = Git::isolated(scratch.path()).expect("the isolated git home");
    let version = git.version().expect("git is installed (PLAN §2.4)");
    eprintln!("{version}");
    (scratch, git)
}

/// Prefixes `b` with CR LF in place: within its buffer when it has room for the prefix, as every input built here
/// has, so a large input is never copied to a second buffer.
fn prepend_crlf(b: &mut Vec<u8>) {
    let n = b.len();
    b.reserve_exact(CRLF.len());
    b.extend_from_slice(CRLF);
    b.copy_within(0..n, CRLF.len());
    b[..CRLF.len()].copy_from_slice(CRLF);
}

/// A text of `len` bytes of 64-byte lines ending in CR LF, in a buffer with room for `spare` more bytes and the CR LF
/// prefix, so neither a tail nor the prefix reallocates it. The buffer size is a multiple of 64, so a pair straddles
/// every multiple of the reader's buffer below `len`: its CR is the last byte of one buffer and its LF the first of
/// the next ([F20 §2.4] "Buffers never change a result").
fn crlf_across_buffers(len: usize, spare: usize) -> Vec<u8> {
    const _: () = assert!(BUF_SIZE.is_multiple_of(64));
    let mut b = Vec::with_capacity(len + spare + CRLF.len());
    b.extend((0..len).map(|i| b'a' + (i % 23) as u8));
    for i in (0..len).step_by(64).skip(1) {
        b[i - 1] = b'\r';
        b[i] = b'\n';
    }
    b
}

/// A named input of the corpus, built only when it is checked, so one input is held at a time.
type Case = (&'static str, Box<dyn Fn() -> Vec<u8>>);

fn case(name: &'static str, make: impl Fn() -> Vec<u8> + 'static) -> Case {
    (name, Box::new(make))
}

/// `head` followed by `tail`, in a buffer with room for the CR LF prefix.
fn joined(head: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(head.len() + tail.len() + CRLF.len());
    b.extend_from_slice(head);
    b.extend_from_slice(tail);
    b
}

/// `base` followed by `tail`, which the room `base` was built with holds.
fn with(mut base: Vec<u8>, tail: &[u8]) -> Vec<u8> {
    base.extend_from_slice(tail);
    base
}

/// The room [`at_ratio`] needs after a text of `len` bytes for `past` controls past the ratio: at most `len >> 7`
/// controls reach the ratio.
const fn ratio_room(len: usize, past: usize) -> usize {
    (len >> 7) + past
}

/// `text` followed by as many `01` controls as put it exactly at the printable ratio of [F20 §2.1]
/// (`printable >> 7 = nonprintable`, text), and `past` more (binary). `text` has the room of [`ratio_room`].
fn at_ratio(mut text: Vec<u8>, past: usize) -> Vec<u8> {
    let st = stats(&text);
    let room = usize::try_from((st.printable >> 7) - st.nonprintable).expect("fits");
    text.reserve_exact(room + past + CRLF.len());
    text.extend(std::iter::repeat_n(0x01, room + past));
    text
}

/// A text with a `1A` as the last byte of its first buffer (not the content's last byte, so it is counted as
/// non-printable, [F20 §2.4]), then the controls of [`at_ratio`].
fn sub_at_first_buffer_end(past: usize) -> Vec<u8> {
    let mut b = crlf_across_buffers(2 * BUF_SIZE, ratio_room(2 * BUF_SIZE, past));
    // BUF_SIZE − 1 holds a CR of a pair; the LF after it stays and is neither printable nor not.
    b[BUF_SIZE - 1] = 0x1a;
    at_ratio(b, past)
}

/// The named synthetic inputs of WP-62's acceptance: CR LF, binary, `^Z`, empty and large, and the buffer edges of
/// [F20 §2.4].
fn corpus() -> Vec<Case> {
    let fixed: &[(&'static str, &'static [u8])] = &[
        ("empty", b""),
        ("hello CR LF (F20 §2.3 table)", b"hello\r\n"),
        ("CR LF inside (F20 §2.3 table)", b"a\r\nb"),
        ("LF only", b"a\nb\n"),
        ("mixed line ends", b"a\r\nb\nc\r\n"),
        ("only CR LF", b"\r\n"),
        ("lone CR (F20 §2.3 table)", b"a\rb"),
        ("lone CR at the end", b"a\r\nb\r"),
        ("LF then CR", b"a\n\rb\r\n"),
        ("CR CR LF", b"a\r\r\nb"),
        ("final ^Z (F20 §2.3 table)", b"x\r\n\x1a"),
        ("two final ^Z", b"x\r\n\x1a\x1a"),
        ("^Z inside", b"x\x1a\r\ny"),
        ("only ^Z", b"\x1a"),
        ("CR LF and final ^Z", b"\r\n\x1a"),
        ("NUL", b"a\0b\r\n"),
        ("DEL", b"a\x7f\r\n"),
        ("BS HT FF ESC are printable", b"\x08\x09\x0c\x1b\r\n"),
        ("UTF-8 BOM", b"\xef\xbb\xbfx\r\n"),
        ("UTF-16 LE BOM", b"\xff\xfea\0\r\0\n\0"),
        ("invalid UTF-8", b"\xff\xfe\xc3\r\n\x80"),
    ];
    let mut v: Vec<Case> = fixed
        .iter()
        .map(|&(name, bytes)| case(name, move || joined(bytes, b"")))
        .collect();
    v.push(case("^Z inside, 128 printable", || {
        joined(&[b'p'; 128], b"\x1a\r\nq\x1a")
    }));
    v.push(case("127 printable and one control", || {
        joined(&[b'A'; 127], b"\r\n\x01")
    }));
    v.push(case("128 printable and one control", || {
        joined(&[b'A'; 128], b"\r\n\x01")
    }));
    v.push(case("buffer size − 1", || {
        crlf_across_buffers(BUF_SIZE - 1, 0)
    }));
    v.push(case("buffer size", || crlf_across_buffers(BUF_SIZE, 0)));
    v.push(case("buffer size + 1", || {
        crlf_across_buffers(BUF_SIZE + 1, 0)
    }));
    // A lone CR, and a final 1A, as the last byte of a full buffer at the end of the content.
    v.push(case("two buffers ending in a lone CR", || {
        with(crlf_across_buffers(2 * BUF_SIZE - 1, 1), b"\r")
    }));
    v.push(case("two buffers ending in ^Z", || {
        with(crlf_across_buffers(2 * BUF_SIZE - 1, 1), b"\x1a")
    }));
    // A lone CR as the last byte of a buffer, its next byte (the next buffer's first) not an LF.
    v.push(case("lone CR across buffers", || {
        let mut b = crlf_across_buffers(BUF_SIZE + 100, 0);
        b[BUF_SIZE] = b'a';
        b
    }));
    // A 1A as the last byte of a buffer that is not the last one: counted, so at the ratio it is text and one control
    // more is binary (a reader that took it for the final 1A would call the second text).
    v.push(case("^Z at a buffer's end, at the printable ratio", || {
        sub_at_first_buffer_end(0)
    }));
    v.push(case(
        "^Z at a buffer's end, one control past the ratio",
        || sub_at_first_buffer_end(1),
    ));
    let big = if common::tier() == "pr" {
        3 << 20
    } else {
        16 << 20
    };
    v.push(case("large, CR LF across buffers", move || {
        crlf_across_buffers(big, 0)
    }));
    v.push(case("large, one NUL at the end", move || {
        with(crlf_across_buffers(big, 1), b"\0")
    }));
    v.push(case("large, lone CR at the end", move || {
        with(crlf_across_buffers(big, 1), b"\r")
    }));
    v.push(case("large, final ^Z", move || {
        with(crlf_across_buffers(big, 1), b"\x1a")
    }));
    // Exactly at the ratio: n printable bytes and n >> 7 controls is text; one control more is binary.
    v.push(case("large, at the printable ratio", || {
        at_ratio(crlf_across_buffers(1 << 20, ratio_room(1 << 20, 0)), 0)
    }));
    v.push(case("large, one control past the ratio", || {
        at_ratio(crlf_across_buffers(1 << 20, ratio_room(1 << 20, 1)), 1)
    }));
    v
}

#[test]
fn oid_equals_git_hash_object_on_the_synthetic_corpus() {
    let (scratch, git) = setup("oid-corpus");
    let mut d = Differential::new(&git, scratch.path());
    let mut failures = Vec::new();
    for (name, make) in corpus() {
        let mut bytes = make();
        let len = bytes.len();
        if let Err(e) = d.check_both(&mut bytes) {
            failures.push(format!("{name} ({len} bytes): {e}"));
        }
    }
    let seen = d.finish();
    eprintln!("oid differential, corpus: {seen:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(seen.inputs as usize, corpus().len());
    // The corpus reaches every class: converted, text left unchanged, and binary.
    assert!(
        seen.converted > 0 && seen.text_unchanged > 0 && seen.binary > 0,
        "{seen:?}"
    );
}

#[test]
fn corpus_inputs_are_built_at_their_final_size() {
    for (name, make) in corpus() {
        let mut b = make();
        let (len, cap) = (b.len(), b.capacity());
        // Room for the prefix, and no doubling from a tail appended after the fact.
        assert!(cap >= len + CRLF.len(), "{name}: {len} bytes in {cap}");
        assert!(cap <= len + len / 64 + 16, "{name}: {len} bytes in {cap}");
        let ptr = b.as_ptr();
        prepend_crlf(&mut b);
        assert_eq!(b.as_ptr(), ptr, "{name}: the prefix moved the buffer");
        assert_eq!((b.len(), &b[..CRLF.len()]), (len + CRLF.len(), CRLF));
    }
    let mut b = b"xyz".to_vec();
    prepend_crlf(&mut b);
    assert_eq!(b, b"\r\nxyz");
    let mut e = Vec::new();
    prepend_crlf(&mut e);
    assert_eq!(e, CRLF);
}

/// Bytes weighted towards the ones [F20 §2.1] classifies: CR, LF, NUL, `^Z`, DEL, the printable controls and the
/// other controls, and bytes ≥ `80`.
fn byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        6 => Just(b'\r'),
        6 => Just(b'\n'),
        1 => Just(0u8),
        2 => Just(0x1au8),
        1 => Just(0x7fu8),
        2 => prop::sample::select(vec![0x08u8, 0x09, 0x0c, 0x1b]),
        1 => 0x01u8..0x20,
        40 => 0x20u8..0x7f,
        6 => 0x80u8..=0xff,
    ]
}

/// Lines of printable text ending all in LF, all in CR LF or in either, sometimes with a stray control (a lone CR
/// among them) or a final `^Z`.
fn text_like() -> impl Strategy<Value = Vec<u8>> {
    (
        0u8..3,
        prop::collection::vec(("[ -~]{0,40}", any::<bool>()), 0..40),
        prop::option::weighted(0.1, 0x01u8..0x20),
        any::<bool>(),
    )
        .prop_map(|(mode, lines, control, final_sub)| {
            let mut b = Vec::new();
            for (line, pick) in lines {
                b.extend_from_slice(line.as_bytes());
                let crlf = mode == 1 || (mode == 2 && pick);
                b.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
            }
            if let Some(c) = control {
                let at = b.len() / 2;
                b.insert(at, c);
            }
            if final_sub {
                b.push(0x1a);
            }
            b
        })
}

#[test]
fn oid_equals_git_hash_object_on_generated_inputs() {
    let (scratch, git) = setup("oid-generated");
    let d = RefCell::new(Differential::new(&git, scratch.path()));
    let inputs = prop_oneof![
        prop::collection::vec(byte(), 0..600),
        text_like(),
        (prop::collection::vec(byte(), 0..64), 1usize..4).prop_map(|(tail, n)| {
            // Around a buffer boundary, so a CR LF can straddle it.
            let mut b = crlf_across_buffers(n * BUF_SIZE - 32, tail.len());
            b.extend(tail);
            b
        }),
    ];
    common::runner("oid_equals_git_hash_object_on_generated_inputs", 96)
        .run(&inputs, |mut bytes| {
            d.borrow_mut()
                .check_both(&mut bytes)
                .map_err(TestCaseError::fail)
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let seen = d.into_inner().finish();
    eprintln!("oid differential, generated: {seen:?}");
    assert!(seen.converted > 0 && seen.binary > 0, "{seen:?}");
}

#[test]
fn scratch_repositories_have_the_object_format_asked_for() {
    let (scratch, git) = setup("oid-formats");
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        let repo = git
            .init(&scratch.path().join(format.algo().name()), format)
            .expect("a scratch repository");
        assert_eq!(repo.format(), format);
        assert_eq!(repo.object_format().expect("rev-parse"), format);
        // The empty blob of [F20 §2.3]'s table, and its SHA-256 counterpart.
        std::fs::write(repo.dir().join("e"), b"").expect("scratch");
        let mut h = repo.hasher(Conversion::TextAuto).expect("hash-object");
        let empty = h.hash("e").expect("an id");
        h.finish().expect("clean exit");
        assert_eq!(empty, oid_of(format, b""));
        let want = match format {
            ObjectFormat::Sha1 => "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
            ObjectFormat::Sha256 => {
                "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813"
            }
        };
        assert_eq!(empty.to_string(), want);
    }
}

#[test]
fn a_missing_file_stops_hash_object_with_its_message() {
    let (scratch, git) = setup("oid-missing");
    let repo = git
        .init(&scratch.path().join("r"), ObjectFormat::Sha1)
        .expect("a scratch repository");
    let mut h = repo.hasher(Conversion::Raw).expect("hash-object");
    match h.hash("absent.bin") {
        // git's own message, read from its standard error: the client's text also names the path, in `args`.
        Err(GitError::Failed { stderr, .. }) => {
            assert!(
                stderr.contains("absent.bin"),
                "git's standard error: {stderr:?}"
            );
        }
        other => panic!("expected git to stop with its message, got {other:?}"),
    }
    assert!(
        h.hash("absent.bin").is_err(),
        "a stopped process answers nothing"
    );
}

#[test]
fn the_corpus_reaches_the_buffer_edges() {
    let get = |name: &str| {
        let (_, make) = corpus()
            .into_iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("no case {name}"));
        make()
    };
    let b = get("two buffers ending in a lone CR");
    assert_eq!((b.len(), b[BUF_SIZE * 2 - 1]), (2 * BUF_SIZE, b'\r'));
    assert_eq!(b[BUF_SIZE * 2 - 2], b'a' + ((BUF_SIZE * 2 - 2) % 23) as u8);
    let b = get("two buffers ending in ^Z");
    assert_eq!((b.len(), b[BUF_SIZE * 2 - 1]), (2 * BUF_SIZE, 0x1a));
    let b = get("lone CR across buffers");
    assert_eq!((b[BUF_SIZE - 1], b[BUF_SIZE]), (b'\r', b'a'));
    assert!(!stats(&b).is_text());
    let text = get("^Z at a buffer's end, at the printable ratio");
    let binary = get("^Z at a buffer's end, one control past the ratio");
    assert_eq!(text[BUF_SIZE - 1], 0x1a);
    assert!(text.len() > 2 * BUF_SIZE && *text.last().expect("non-empty") == 0x01);
    assert!(stats(&text).is_text());
    assert!(!stats(&binary).is_text());
}
