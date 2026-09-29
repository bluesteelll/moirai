//! `fixtures/hex` (WP-20): every `.bin` decodes into typed values and re-encodes byte-identically ([PLAN §6.2] R3:
//! compressed payloads are opaque), and the conclusion each file's first comment states holds: the slot a `HEAD`
//! selects or why none is ([F04 §8.1]); where and why a log's scan ends ([F05 §5.3]), what lies after that point, and
//! that the `HEAD` is the fold of the log it covers ([F04 §6], [F05 §10.2]); that a sealed file's header matches its name
//! ([F09 §17.1] V-3) and every reference in its store that names it (V-8); and that a fragment decodes to its end.

use std::collections::BTreeMap;

use moirai_format_oracle::fixture::{self, FileName, Fragment, Kind, Sealed};
use moirai_format_oracle::head::{Choice, HeadSlot, Refusal};
use moirai_format_oracle::log::{Payload, Scan, Stop};
use moirai_format_oracle::runtime::{self, FV, SegKind, Table};
use moirai_format_oracle::segment;

use super::common::{family, read, rel, run_all, walk};

/// The slot selection a `HEAD` fixture states ([F04 §8.1]).
#[derive(Clone, Copy, Debug)]
enum Sel {
    /// Slot A.
    A,
    /// Slot B.
    B,
    /// A slot is selected; the file does not say which.
    Some,
    /// No valid slot: read again, then exit 7.
    None,
    /// The slot fails check n of [F04 §7] after its checksum matched: exit 7.
    Fatal(Choice, u8),
    /// Both slots valid with equal `slot_seq` and different bytes: exit 7.
    EqualSeq,
    /// Both slots valid with different init blocks ([F17 §2.2] IP-3): exit 7.
    InitMismatch,
}

/// Where a log's scan stops ([F05 §5.3]).
#[derive(Clone, Copy, Debug)]
enum End {
    /// At the end of the valid log, at or above `committed_lsn`, with zeros after it ([F05 §2.4]).
    Zero,
    /// At the end of the valid log at this lsn, at or above `durable_lsn`, with stale bytes after it.
    At(u64),
    /// Corruption below `durable_lsn` at this lsn: exit 7.
    Corrupt(u64),
}

/// The records after a scan's end that decode detached and that do not ([`fixture::walk_tail`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tail {
    /// Records that decode and re-encode byte for byte.
    decoded: usize,
    /// Records that fail §5.2 or §5.4 on their own.
    refused: usize,
}

/// Nothing after the end of the valid log.
const NO_TAIL: Tail = Tail {
    decoded: 0,
    refused: 0,
};

/// What a hex fixture is and what it must yield.
#[derive(Clone, Debug)]
enum Expect {
    /// A whole file with a magic: decode, re-encode byte-identically, and match its name (V-3).
    File(Kind),
    /// A `HEAD`: decode, re-encode every slot that can be, and select.
    Head(Sel),
    /// Log extents scanned with the `HEAD` their store publishes (path of that `HEAD`, then the other extents), where
    /// the scan ends, and what lies after it.
    Log(&'static str, &'static [&'static str], End, Tail),
    /// A sequence of one structure to the end of the file.
    Fragment(Fragment),
}

use End::*;
use Expect::*;

/// Every `.bin` of `fixtures/hex`, with its expectation from its first comment block and `fixtures/hex/INDEX.md` §3.
fn table() -> Vec<(&'static str, Expect)> {
    let mut t = vec![
        ("chain/corrupt/HEAD.bin", Head(Sel::Some)),
        (
            "chain/corrupt/log.1.bin",
            // log.1.hex: c2's group g4 at lsn 597 carries the flipped bit, so its record fails its checksum; c3's
            // valid group after it decodes on its own and repairs nothing (INDEX.md §3.5).
            Log(
                "chain/corrupt/HEAD.bin",
                &[],
                Corrupt(597),
                Tail {
                    decoded: 1,
                    refused: 1,
                },
            ),
        ),
        ("chain/lazy-tail/HEAD.bin", Head(Sel::Some)),
        (
            "chain/lazy-tail/log.1.bin",
            // L2 holds twelve cursor records; the crash lost its last 512-byte sector, so the first eleven records are
            // whole and the twelfth, with the chain trailer, fails its checksum (log.1.hex, INDEX.md §3.5).
            Log(
                "chain/lazy-tail/HEAD.bin",
                &[],
                At(680),
                Tail {
                    decoded: 11,
                    refused: 1,
                },
            ),
        ),
        ("chain/re-roll/HEAD.bin", Head(Sel::Some)),
        (
            "chain/re-roll/log.2.bin",
            Log("chain/re-roll/HEAD.bin", &[], Zero, NO_TAIL),
        ),
        ("chain/wrong-position/HEAD.bin", Head(Sel::Some)),
        (
            "chain/wrong-position/log.1.bin",
            // The record at p says lsn p + 64: invalid at p, but whole on its own (INDEX.md §3.5).
            Log(
                "chain/wrong-position/HEAD.bin",
                &[],
                At(597),
                Tail {
                    decoded: 1,
                    refused: 0,
                },
            ),
        ),
        ("codec/blobs.2.bin", File(Kind::Blobs)),
        ("codec/blobs.5.bin", File(Kind::Blobs)),
        ("codec/blobs.7.bin", File(Kind::Blobs)),
        ("codec/dict.1.bin", File(Kind::Dict)),
        ("codec/dict.6.bin", File(Kind::Dict)),
        ("codec/hist.3.bin", File(Kind::Hist)),
        ("codec/hist.4.bin", File(Kind::Hist)),
        ("decode-only/journal-cursor/HEAD.bin", Head(Sel::Some)),
        (
            "decode-only/journal-cursor/log.1.bin",
            Log("decode-only/journal-cursor/HEAD.bin", &[], Zero, NO_TAIL),
        ),
        (
            "fragments/anchors/records.bin",
            Fragment(Fragment::AnchorRecords),
        ),
        (
            "fragments/binding/binding-ext.bin",
            Fragment(Fragment::BindingExts),
        ),
        (
            "fragments/binding/clienthead-payload.bin",
            // [F05 §8.1] class 5 `git-branch`, symbol 7 = `u/l5np` (the fragment's first comment).
            Fragment(Fragment::ClientHeadPayload {
                defined: vec![(5, 7)],
            }),
        ),
        (
            "fragments/binding/heads-row.bin",
            Fragment(Fragment::HeadsRow),
        ),
        (
            "fragments/ops/variants.bin",
            Fragment(Fragment::OpFrames { lsn: 30_000 }),
        ),
        (
            "fragments/runtime/os-ids.bin",
            Fragment(Fragment::OsIds([5, 5, 5])),
        ),
        (
            "fragments/schema/items.bin",
            Fragment(Fragment::SchemaItems),
        ),
        (
            "fragments/values/cv.bin",
            Fragment(Fragment::CanonicalValues),
        ),
        (
            "fragments/values/field-block.bin",
            Fragment(Fragment::FieldBlocks),
        ),
        (
            "fragments/values/op-values.bin",
            Fragment(Fragment::OpValues),
        ),
        ("head/absent-zero.bin", Head(Sel::A)),
        ("head/fatal/equal-seq.bin", Head(Sel::EqualSeq)),
        ("head/fatal/format-2.bin", Head(Sel::Fatal(Choice::B, 3))),
        ("head/fatal/init-mismatch.bin", Head(Sel::InitMismatch)),
        ("head/fatal/range.bin", Head(Sel::Fatal(Choice::B, 5))),
        (
            "head/fatal/reserved-bit.bin",
            Head(Sel::Fatal(Choice::B, 4)),
        ),
        ("head/flags/boot-change.bin", Head(Sel::B)),
        ("head/flags/readonly.bin", Head(Sel::B)),
        ("head/flags/retired.bin", Head(Sel::B)),
        ("head/init.bin", Head(Sel::B)),
        ("head/two-slot/new-new.bin", Head(Sel::B)),
        ("head/two-slot/new-old.bin", Head(Sel::A)),
        ("head/two-slot/new-torn.bin", Head(Sel::A)),
        ("head/two-slot/old-new.bin", Head(Sel::B)),
        ("head/two-slot/old-old.bin", Head(Sel::B)),
        ("head/two-slot/old-torn.bin", Head(Sel::A)),
        ("head/two-slot/torn-new.bin", Head(Sel::B)),
        ("head/two-slot/torn-old.bin", Head(Sel::B)),
        ("head/two-slot/torn-torn.bin", Head(Sel::None)),
        ("lock/init.bin", File(Kind::Lock)),
        ("lock/live.bin", File(Kind::Lock)),
        ("store-a/blobs.2.bin", File(Kind::Blobs)),
        ("store-a/blobs.9.bin", File(Kind::Blobs)),
        ("store-a/cs.6.bin", File(Kind::Segment)),
        ("store-a/gitmap.3.bin", File(Kind::Gitmap)),
        ("store-a/gitmap.4.bin", File(Kind::Gitmap)),
        // Store A's HEAD is in hex/head/: every two-slot state is "store A at the end of its log.1 fixture".
        (
            "store-a/log.1.bin",
            Log("head/two-slot/new-new.bin", &[], Zero, NO_TAIL),
        ),
        ("store-a/seg.b1.5.bin", File(Kind::Segment)),
        ("store-a/seg.base.10.bin", File(Kind::Segment)),
        ("store-a/seg.d1.bin", File(Kind::Segment)),
        ("store-a/seg.d7.bin", File(Kind::Segment)),
        ("store-a/seg.d8.bin", File(Kind::Segment)),
        ("store-b/HEAD.bin", Head(Sel::Some)),
        ("store-b/blobs.4.bin", File(Kind::Blobs)),
        ("store-b/hist.2.bin", File(Kind::Hist)),
        ("store-b/hist.5.bin", File(Kind::Hist)),
        // Both extents in one scan: log.2's ExtentHead carries log.1's pad trailer ([F05 §4.5]).
        (
            "store-b/log.1.bin",
            Log("store-b/HEAD.bin", &["store-b/log.2.bin"], Zero, NO_TAIL),
        ),
        ("store-b/seg.d3.bin", File(Kind::Segment)),
        ("unknown-boot/HEAD.bin", Head(Sel::Some)),
        ("unknown-boot/LOCK.bin", File(Kind::Lock)),
        (
            "unknown-boot/log.1.bin",
            Log("unknown-boot/HEAD.bin", &[], Zero, NO_TAIL),
        ),
    ];
    t.sort_by_key(|x| x.0);
    t
}

/// Fixtures whose conclusion differs from the oracle's reading of the specification (reported as spec findings; each
/// has an ignored test below).
const KNOWN: &[&str] = &[
    "store-a/seg.b1.5.bin",
    "store-a/seg.base.10.bin",
    "store-a/seg.d1.bin",
];

/// The slot a store's `HEAD` selects ([F04 §8.1]), after its own decode and re-encode.
fn head_of(path: &str) -> Result<HeadSlot, String> {
    let h = fixture::check_head(&read(&family("hex").join(path)))
        .map_err(|e| format!("{path}: {e}"))?;
    let (_, s) = h
        .choose()
        .map_err(|e| format!("{path}: no slot to use: {e}"))?;
    Ok(s.clone())
}

/// The file name of a fixture path without `.bin` (the store file it stands for, [F02 §6.3]).
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".bin").unwrap_or(name)
}

/// [F04 §5.12]: every valid `seq_ring` entry (s, lsn) whose lsn the scan covers names the `Commit` record with `seq` s
/// there; the ring is log-derived (§6), so a `HEAD` and the log it folds agree.
fn seq_ring_names_commits(slot: &HeadSlot, s: &Scan) -> Result<(), String> {
    let Some(start) = s.groups.first().map(|g| g.lsn) else {
        return Ok(());
    };
    for (k, e) in slot.seq_ring.iter().enumerate() {
        let valid = e.seq != 0
            && e.seq % 32 == k as u64
            && e.seq <= slot.commit_seq
            && e.seq + 32 > slot.commit_seq;
        if !valid || e.lsn < start || e.lsn >= s.end {
            continue;
        }
        let rec = s
            .groups
            .iter()
            .flat_map(|g| &g.records)
            .find(|r| r.hdr.lsn == e.lsn);
        match rec.map(|r| &r.payload) {
            Some(Payload::Commit(c)) if c.seq == e.seq => {}
            other => {
                return Err(format!(
                    "HEAD seq_ring[{k}] = (seq {}, lsn {}) names no Commit of that seq in the log (found {})",
                    e.seq,
                    e.lsn,
                    match other {
                        Some(Payload::Commit(c)) => format!("the Commit of seq {}", c.seq),
                        Some(p) => format!("record kind {}", p.kind()),
                        None => "no record boundary".into(),
                    }
                ));
            }
        }
    }
    Ok(())
}

/// The extents of a log fixture as (n, bytes), `path` first.
fn extents(path: &str, more: &[&str]) -> Result<Extents, String> {
    let base = family("hex");
    let mut ext = Vec::new();
    for p in std::iter::once(path).chain(more.iter().copied()) {
        let n: u32 = stem(p)
            .strip_prefix("log.")
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| format!("{p} is not named log.<n>"))?;
        ext.push((n, read(&base.join(p))));
    }
    Ok(ext)
}

/// A log's extents as (n, bytes of log.<n>).
type Extents = Vec<(u32, Vec<u8>)>;

/// The scan of a store's log with the slot its `HEAD` selects, and the extents scanned.
fn scan_of(
    head: &str,
    path: &str,
    more: &[&str],
) -> Result<(HeadSlot, fixture::LogCheck, Extents), String> {
    let slot = head_of(head)?;
    let ext = extents(path, more)?;
    let refs: Vec<(u32, &[u8])> = ext.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let lc = fixture::check_log(&refs, &slot).map_err(|e| e.to_string())?;
    Ok((slot, lc, ext))
}

fn check(path: &str, expect: &Expect) -> Result<(), String> {
    let base = family("hex");
    let b = read(&base.join(path));
    match expect {
        File(k) => {
            let got = fixture::check_file(&b, Some(stem(path))).map_err(|e| e.to_string())?;
            if got != *k {
                return Err(format!("decoded as {got:?}, not {k:?}"));
            }
        }
        Head(sel) => {
            let h = fixture::check_head(&b).map_err(|e| e.to_string())?;
            match (h.choose(), sel) {
                (Ok((Choice::A, _)), Sel::A | Sel::Some)
                | (Ok((Choice::B, _)), Sel::B | Sel::Some)
                | (Err(Refusal::NoValidSlot), Sel::None)
                | (Err(Refusal::EqualSeq), Sel::EqualSeq)
                | (Err(Refusal::InitMismatch), Sel::InitMismatch) => {}
                (Err(Refusal::Fatal(c, f)), Sel::Fatal(want_c, want_check))
                    if c == *want_c && f.check == *want_check => {}
                (got, _) => return Err(format!("selection {got:?}, the fixture states {sel:?}")),
            }
        }
        Log(head, more, end, tail) => {
            let (slot, lc, ext) = scan_of(head, path, more)?;
            let s = &lc.scan;
            match (end, &s.stop) {
                (Zero, Stop::End(_) | Stop::Clean) => {
                    if !lc.zero_tail {
                        return Err(format!(
                            "bytes after the end of the valid log {} are not zero",
                            s.end
                        ));
                    }
                    if s.end < slot.committed_lsn {
                        return Err(format!(
                            "the valid log ends at {} below committed_lsn {}",
                            s.end, slot.committed_lsn
                        ));
                    }
                }
                (At(p), Stop::End(_)) if s.end == *p && *p >= slot.durable_lsn => {}
                (Corrupt(p), Stop::Corrupt(_)) if s.end == *p && *p < slot.durable_lsn => {}
                (_, stop) => {
                    return Err(format!(
                        "the scan stopped at {} ({stop:?}), the fixture states {end:?}",
                        s.end
                    ));
                }
            }
            seq_ring_names_commits(&slot, s)?;
            fixture::check_head_fold(&slot, s).map_err(|e| e.to_string())?;
            let e = slot.init.log_extent_bytes;
            let n = u32::try_from(s.end / e + 1).unwrap_or(u32::MAX);
            let got = match ext.iter().find(|x| x.0 == n) {
                Some((_, bytes)) => {
                    let recs = fixture::walk_tail(bytes, (s.end % e) as usize)
                        .map_err(|e| e.to_string())?;
                    Tail {
                        decoded: recs.iter().filter(|r| r.decoded.is_ok()).count(),
                        refused: recs.iter().filter(|r| r.decoded.is_err()).count(),
                    }
                }
                None => NO_TAIL,
            };
            if got != *tail {
                return Err(format!(
                    "after the end of the scan: {got:?}, the fixture states {tail:?}"
                ));
            }
        }
        Fragment(f) => {
            fixture::check_fragment(f, &b).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn check_named(path: &str) -> Result<(), String> {
    let t = table();
    let (_, e) = t
        .iter()
        .find(|x| x.0 == path)
        .ok_or_else(|| format!("{path} is not in the table"))?;
    check(path, e)
}

/// E3 over every hex fixture but the known mismatches.
#[test]
fn hex_fixtures() {
    run_all(
        "fixtures/hex",
        &table(),
        |x| x.0.to_owned(),
        KNOWN,
        |x| check(x.0, &x.1),
    );
}

/// Every `.bin` is in the table (as a fixture or an extent of one) and every `.hex` has its assembled `.bin`.
#[test]
fn hex_table_is_complete() {
    let base = family("hex");
    let t = table();
    let mut named: Vec<&str> = t.iter().map(|x| x.0).collect();
    for (_, e) in &t {
        if let Log(h, more, _, _) = e {
            named.push(h);
            named.extend(more.iter());
        }
    }
    let mut missing = Vec::new();
    for p in walk(&base) {
        let r = rel(&p, &base);
        match p.extension().and_then(|e| e.to_str()) {
            Some("bin") if !named.contains(&r.as_str()) => {
                missing.push(format!("{r}: not in the table"))
            }
            Some("hex") if !p.with_extension("bin").exists() => missing.push(format!(
                "{r}: no assembled .bin beside it (cargo xtask hex)"
            )),
            _ => {}
        }
    }
    for n in &named {
        if !base.join(n).exists() {
            missing.push(format!("{n}: in the table but not on disk"));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// One reference to a sealed file ([F09 §17.1] V-8): who holds it and what it states.
struct Reference {
    /// Where the reference is (for the failure text).
    holder: String,
    /// The file it names.
    file: FileName,
    /// `total_len`, when the reference states it.
    total_len: Option<u64>,
    /// The 16-byte digest, when the reference states it.
    digest16: Option<[u8; 16]>,
    /// `upto_lsn`, when the reference states it.
    upto_lsn: Option<u64>,
    /// `from_lsn`, when the reference states it.
    from_lsn: Option<u64>,
}

impl Reference {
    fn new(holder: String, file: FileName) -> Reference {
        Reference {
            holder,
            file,
            total_len: None,
            digest16: None,
            upto_lsn: None,
            from_lsn: None,
        }
    }
}

/// The family of a `SegRef.kind` ([F04 §4.1]: 1 base, 2 delta, 3 dict).
fn segref_file(s: &moirai_format_oracle::head::SegRef) -> FileName {
    FileName {
        family: match s.kind {
            1 => 3,
            2 => 4,
            _ => 7,
        },
        ref_id: 0,
        file_no: s.file_no,
    }
}

fn segref_reference(holder: String, s: &moirai_format_oracle::head::SegRef) -> Reference {
    let mut r = Reference::new(holder, segref_file(s));
    r.digest16 = Some(s.blake3_16);
    if s.kind != 3 {
        r.upto_lsn = Some(s.upto_lsn);
    }
    r
}

/// The references a store's `HEAD` slot and scanned log hold ([F04 §4.1], [F05 §9.1] `cs_ref`, §9.9).
fn log_references(slot: &HeadSlot, scan: &Scan, head: &str) -> Vec<Reference> {
    let mut v: Vec<Reference> = slot.segments[..usize::from(slot.n_segments)]
        .iter()
        .map(|s| segref_reference(format!("{head} SegRef"), s))
        .collect();
    for rec in scan.groups.iter().flat_map(|g| &g.records) {
        let at = format!("the record at lsn {}", rec.hdr.lsn);
        match &rec.payload {
            Payload::Commit(c) => {
                if let Some((f, len, d)) = c.cs_ref {
                    let mut r = Reference::new(
                        format!("{at} (cs_ref)"),
                        FileName {
                            family: 9,
                            ref_id: 0,
                            file_no: f,
                        },
                    );
                    r.total_len = Some(len);
                    r.digest16 = Some(d);
                    v.push(r);
                }
            }
            Payload::Checkpoint(ck) => {
                if let Some((_, _, segs)) = &ck.set {
                    v.extend(
                        segs.iter()
                            .map(|s| segref_reference(format!("{at} (set)"), s)),
                    );
                }
                for p in ck.promotions.iter().flatten() {
                    let mut r = Reference::new(
                        format!("{at} (Promotion)"),
                        FileName {
                            family: 5,
                            ref_id: p.ref_id,
                            file_no: p.seg_file,
                        },
                    );
                    r.total_len = Some(p.total_len);
                    r.digest16 = Some(p.digest);
                    v.push(r);
                }
                for t in ck.retirements.iter().flatten() {
                    let mut r = Reference::new(
                        format!("{at} (Retirement)"),
                        FileName {
                            family: 2,
                            ref_id: 0,
                            file_no: t.hist_file,
                        },
                    );
                    r.total_len = Some(t.total_len);
                    r.digest16 = Some(t.digest);
                    v.push(r);
                }
                for e in ck.added.iter().flatten() {
                    let mut r = Reference::new(
                        format!("{at} (FileEntry)"),
                        FileName {
                            family: e.file.family,
                            ref_id: e.file.ref_id,
                            file_no: e.file.file_no,
                        },
                    );
                    r.total_len = Some(e.total_len);
                    r.digest16 = Some(e.digest);
                    v.push(r);
                }
            }
            _ => {}
        }
    }
    v
}

/// The rows of a segment's `FILES` section ([F09 §14.4]) as references, read from the section alone.
fn files_references(name: &str, b: &[u8]) -> Result<Vec<Reference>, String> {
    if !b.starts_with(b"MSEG") {
        return Ok(Vec::new());
    }
    let c = segment::decode_container(b).map_err(|e| format!("{name}: {e}"))?;
    let seg = match c.hdr.seg_kind {
        3 => SegKind::Base,
        4 => SegKind::Delta,
        _ => return Ok(Vec::new()),
    };
    let Some((_, (bytes, at))) = c
        .entries
        .iter()
        .zip(&c.sections)
        .find(|(e, _)| e.tag == Table::Files.tag())
    else {
        return Ok(Vec::new());
    };
    let s = runtime::decode_section(Table::Files, bytes, *at, seg)
        .map_err(|e| format!("{name} FILES: {e}"))?;
    let mut v = Vec::new();
    for row in &s.rows {
        let FV::FileRef(family, ref_id, file_no) = *row.get("file") else {
            continue;
        };
        let mut r = Reference::new(
            format!("{name} FILES"),
            FileName {
                family,
                ref_id,
                file_no,
            },
        );
        if row.u("flags") & 1 == 0 {
            r.total_len = Some(row.u("total_len"));
            r.digest16 = Some(row.bytes("digest16").try_into().expect("16"));
            if matches!(family, 2 | 5) {
                r.upto_lsn = Some(row.u("upto_lsn"));
            }
            if family == 2 {
                r.from_lsn = Some(row.u("from_lsn"));
            }
        }
        v.push(r);
    }
    Ok(v)
}

/// [F09 §17.1] V-8 over one store: every reference its `HEAD`, its log and its segments' `FILES` sections hold to a
/// sealed file of the store's fixtures states that file's `total_len`, digest and bounds; and every sealed file of the
/// store is named by at least one reference ([F09 §14.4]: `HEAD`, the log and `FILES` name every live file; a released
/// file was named by the log that made it live).
fn check_store(dir: &str, head: &str, log: &str, more: &[&str]) -> Result<(), String> {
    let base = family("hex");
    let mut files: BTreeMap<FileName, (String, Sealed)> = BTreeMap::new();
    let mut refs = Vec::new();
    for p in walk(&base.join(dir)) {
        let r = rel(&p, &base);
        let s = stem(&r);
        if !r.ends_with(".bin") || FileName::parse(s).is_none() {
            continue;
        }
        let b = read(&p);
        let sealed = fixture::sealed_identity(s, &b).map_err(|e| format!("{r}: {e}"))?;
        refs.extend(files_references(&r, &b)?);
        files.insert(sealed.name, (r, sealed));
    }
    let (slot, lc, _) = scan_of(head, log, more)?;
    refs.extend(log_references(&slot, &lc.scan, head));
    let mut named = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for r in &refs {
        let Some((path, f)) = files.get(&r.file) else {
            continue;
        };
        named.insert(r.file);
        let checks = [
            ("total_len", r.total_len, f.total_len),
            ("upto_lsn", r.upto_lsn, f.upto_lsn),
            ("from_lsn", r.from_lsn, f.from_lsn),
        ];
        for (what, got, want) in checks {
            if let Some(g) = got
                && g != want
            {
                failures.push(format!(
                    "{}: {what} {g} for {path}, whose header says {want}",
                    r.holder
                ));
            }
        }
        if let Some(d) = r.digest16
            && d != f.digest16
        {
            failures.push(format!(
                "{}: a digest that is not {path}'s recorded digest",
                r.holder
            ));
        }
    }
    for (n, (path, _)) in &files {
        if !named.contains(n) {
            failures.push(format!(
                "{path}: no HEAD, log record or FILES row of the store names it"
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// [F09 §17.1] V-8: stores A and B reference their sealed files consistently.
#[test]
fn hex_store_references() {
    let mut failures = Vec::new();
    for (dir, head, log, more) in [
        (
            "store-a",
            "head/two-slot/new-new.bin",
            "store-a/log.1.bin",
            &[][..],
        ),
        (
            "store-b",
            "store-b/HEAD.bin",
            "store-b/log.1.bin",
            &["store-b/log.2.bin"][..],
        ),
    ] {
        if let Err(e) = check_store(dir, head, log, more) {
            failures.push(format!("{dir}:\n{e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// [F04 §6], [F05 §10.2]: every valid slot of store A's `HEAD` fixtures (`init`, the two-slot states, the flag
/// publishes, `absent-zero`) is the fold of store A's log up to that slot's `committed_lsn` (INDEX.md §3.3: `init.hex`
/// holds the fold of g1–g2; the other states are store A at the end of `log.1`).
#[test]
fn store_a_heads_are_folds_of_its_log() {
    use moirai_format_oracle::head::SlotClass;
    let base = family("hex");
    let (_, lc, _) = scan_of("head/two-slot/new-new.bin", "store-a/log.1.bin", &[])
        .unwrap_or_else(|e| panic!("store-a/log.1.bin: {e}"));
    let mut failures = Vec::new();
    let mut slots = 0usize;
    for p in walk(&base.join("head")) {
        let r = rel(&p, &base);
        if !r.ends_with(".bin") || r.starts_with("head/fatal/") {
            continue;
        }
        let h = fixture::check_head(&read(&p)).unwrap_or_else(|e| panic!("{r}: {e}"));
        for (which, s) in [("A", &h.a), ("B", &h.b)] {
            if let SlotClass::Valid(slot) = s {
                slots += 1;
                if let Err(e) = fixture::check_head_fold(slot, &lc.scan) {
                    failures.push(format!("{r} slot {which}: {e}"));
                }
            }
        }
    }
    assert!(slots > 0, "no valid slot among the store A HEAD fixtures");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn run_named(path: &str) {
    if let Err(e) = check_named(path) {
        panic!("{path}: {e}");
    }
}

/// Only the FCOL sections of fields the touched rows hold; [F09 §10.1] reads as every FCOL its FPROMO rows imply.
#[test]
#[ignore = "mismatch: hex/store-a/seg.b1.5.bin, awaiting ruling"]
fn mismatch_store_a_seg_b1_5() {
    run_named("store-a/seg.b1.5.bin");
}

/// A non-inline `IDEM` result `HeapRef` at the running offset ([F11 §2.3]) where §8 says zero; no `FCOL`/`FIDX` for
/// `assignee` ([F09 §10.1]).
#[test]
#[ignore = "mismatch: hex/store-a/seg.base.10.bin, awaiting ruling"]
fn mismatch_store_a_seg_base_10() {
    run_named("store-a/seg.base.10.bin");
}

/// As seg.base.10: the non-inline `IDEM` result `HeapRef`, and no `FCOL.assignee`.
#[test]
#[ignore = "mismatch: hex/store-a/seg.d1.bin, awaiting ruling"]
fn mismatch_store_a_seg_d1() {
    run_named("store-a/seg.d1.bin");
}
