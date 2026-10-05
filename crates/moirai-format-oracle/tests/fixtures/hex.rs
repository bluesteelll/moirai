//! `fixtures/hex` (WP-20): every `.bin` decodes into typed values and re-encodes byte-identically ([PLAN §6.2] R3:
//! compressed payloads are opaque), and the conclusion each file's first comment states holds: the slot a `HEAD`
//! selects or why none is ([F04 §8.1]); where and why a log's scan ends ([F05 §5.3]), what lies after that point, and
//! that the `HEAD` is the fold of the log it covers ([F04 §6], [F05 §10.2]); that a sealed file's header matches its name
//! ([F09 §17.1] V-3) and every reference in its store that names it (V-8); that every view a store's log and `HEAD`
//! name passes the set-wide checks (V-12, names resolved through every `SYMTAB` of the set); and that a fragment decodes
//! to its end, its schema items in item key order and every `Schema` op's `item_key` its items' stored key form, with
//! the symbols its first comment names ([F08 §8.5], [F06 §7.6]).

use std::collections::{BTreeMap, BTreeSet};

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

/// A fragment's symbols of class `name`, ids 1, 2, … in order.
fn names(n: &[&str]) -> Vec<String> {
    n.iter().map(|s| (*s).to_owned()).collect()
}

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
            // The fragment's symbols of class `name`, ids 1, 2, … in first-use order (named where used).
            Fragment(Fragment::OpFrames {
                lsn: 30_000,
                names: names(&[
                    "estimate",
                    "owner_quote",
                    "stale_blockers",
                    "incident",
                    "impact",
                    "priority",
                    "P5",
                    "status",
                    "triaged",
                    "escalates_to",
                    "ESCALATES_TO",
                    "escalated_from",
                ]),
            }),
        ),
        (
            "fragments/runtime/os-ids.bin",
            Fragment(Fragment::OsIds([5, 5, 5])),
        ),
        (
            "fragments/schema/items.bin",
            // The fragment's own symbols of class `name`, ids 1, 2, … in first-use order (named where used).
            Fragment(Fragment::SchemaItems {
                names: names(&[
                    "incident",
                    "impact",
                    "pager",
                    "seen_at",
                    "tags",
                    "status",
                    "open",
                    "resolved",
                    "wontfix",
                    "escalates_to",
                    "ESCALATES_TO",
                    "escalated_from",
                    "open_incidents",
                ]),
            }),
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

/// Fixtures whose conclusion differs from the oracle's reading of the specification, reported as spec findings until the
/// ruling lands. The walk runs them as expected failures: one that passes, or names no fixture, fails the walk.
const KNOWN: &[&str] = &[];

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

/// E3 over every hex fixture.
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

/// A ref's `promoted_seg` ([F11 §3.1]) as a reference: it names `seg.b<ref_id>.<K>` by the ref's id and K, and states
/// nothing more of the file.
fn promoted_reference(holder: String, ref_id: u32, k: u32) -> Reference {
    Reference::new(
        holder,
        FileName {
            family: 5,
            ref_id,
            file_no: k,
        },
    )
}

/// The references a store's `HEAD` slot and scanned log hold ([F04 §4.1], [F05 §9.1] `cs_ref`, §9.9, §9.10
/// `promoted_seg`).
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
            Payload::RefTable(entries) => {
                for e in entries {
                    if let Some(u) = e.upsert.as_ref().filter(|u| u.promoted_seg != 0) {
                        v.push(promoted_reference(
                            format!("{at} (RefTable promoted_seg)"),
                            e.ref_id,
                            u.promoted_seg,
                        ));
                    }
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

/// The rows of a segment's `FILES` section ([F09 §14.4]) and the `promoted_seg` of its `REFS` rows ([F11 §3.1]) as
/// references, read from the sections alone.
fn segment_references(name: &str, b: &[u8]) -> Result<Vec<Reference>, String> {
    if !b.starts_with(b"MSEG") {
        return Ok(Vec::new());
    }
    let c = segment::decode_container(b).map_err(|e| format!("{name}: {e}"))?;
    let seg = match c.hdr.seg_kind {
        3 => SegKind::Base,
        4 => SegKind::Delta,
        _ => return Ok(Vec::new()),
    };
    let table = |t: Table| -> Result<Option<runtime::Section>, String> {
        c.entries
            .iter()
            .zip(&c.sections)
            .find(|(e, _)| e.tag == t.tag())
            .map(|(_, (bytes, at))| {
                runtime::decode_section(t, bytes, *at, seg)
                    .map_err(|e| format!("{name} {t:?}: {e}"))
            })
            .transpose()
    };
    let mut v = Vec::new();
    for row in table(Table::Refs)?.iter().flat_map(|s| &s.rows) {
        let k = row.u("promoted_seg") as u32;
        if k != 0 {
            v.push(promoted_reference(
                format!("{name} REFS promoted_seg"),
                row.u("ref_id") as u32,
                k,
            ));
        }
    }
    let Some(s) = table(Table::Files)? else {
        return Ok(v);
    };
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

/// [F09 §17.1] V-8 over one store: every reference its `HEAD`, its log and its segments' `FILES` and `REFS` sections
/// hold to a sealed file of the store's fixtures states that file's `total_len`, digest and bounds (a ref's
/// `promoted_seg`, in a `RefTable` record or a `REFS` row, names `seg.b<ref_id>.<K>` by the ref id and K that the
/// file's header carries, V-3, and states nothing more); and every sealed file of the store is named by at least one
/// reference ([F09 §14.4]: `HEAD`, the log and `FILES` name every live file; a released file was named by the log that
/// made it live).
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
        refs.extend(segment_references(&r, &b)?);
        files.insert(sealed.name, (r, sealed));
    }
    let (slot, lc, _) = scan_of(head, log, more)?;
    refs.extend(log_references(&slot, &lc.scan, head));
    let mut named = BTreeSet::new();
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

/// The base and delta files of a main set ([F04 §4.1]: base first, then the deltas oldest first; the dictionary is no
/// layer).
fn main_set(segs: &[moirai_format_oracle::head::SegRef]) -> Vec<FileName> {
    segs.iter()
        .filter(|s| matches!(s.kind, 1 | 2))
        .map(segref_file)
        .collect()
}

/// [F09 §17.1] V-12 and the set-wide name checks over one store, with the symbols of every `SYMTAB` of its segments
/// ([F09 §17.1] last paragraph): every main set a `Checkpoint` record publishes ([F05 §9.9] bit 0) and the `HEAD`
/// names ([F04 §4.1]); every promoted branch segment on top of the set its `Promotion.base_pin` names ([F05 §9.9]: 0 =
/// the set the record publishes; [F09 §16.3]); and every changeset a bulk commit names ([F06 §9] `cs_ref`).
fn check_views(dir: &str, head: &str, log: &str, more: &[&str]) -> Result<(), String> {
    let base = family("hex");
    let mut segs: BTreeMap<FileName, (String, segment::Segment)> = BTreeMap::new();
    for p in walk(&base.join(dir)) {
        let r = rel(&p, &base);
        let Some(name) = FileName::parse(stem(&r))
            .filter(|n| r.ends_with(".bin") && matches!(n.family, 3 | 4 | 5 | 9))
        else {
            continue;
        };
        let s = segment::decode_segment(&read(&p)).map_err(|e| format!("{r}: {e}"))?;
        segs.insert(name, (r, s));
    }
    let syms = segment::SetSymbols::of(segs.values().map(|x| &x.1)).map_err(|e| e.to_string())?;
    let (slot, lc, _) = scan_of(head, log, more)?;
    let mut stacks: Vec<Vec<FileName>> =
        vec![main_set(&slot.segments[..usize::from(slot.n_segments)])];
    let mut published: BTreeMap<u64, Vec<FileName>> = BTreeMap::new();
    let mut changesets = Vec::new();
    for rec in lc.scan.groups.iter().flat_map(|g| &g.records) {
        match &rec.payload {
            Payload::Checkpoint(ck) => {
                let own = ck.set.as_ref().map(|(_, _, s)| main_set(s));
                if let Some(set) = &own {
                    published.insert(rec.hdr.lsn, set.clone());
                    stacks.push(set.clone());
                }
                for p in ck.promotions.iter().flatten() {
                    let pin = if p.base_pin == 0 {
                        own.as_ref()
                    } else {
                        published.get(&p.base_pin)
                    };
                    let mut st = pin
                        .ok_or_else(|| {
                            format!(
                                "the Promotion at lsn {} pins a set no scanned Checkpoint publishes",
                                rec.hdr.lsn
                            )
                        })?
                        .clone();
                    st.push(FileName {
                        family: 5,
                        ref_id: p.ref_id,
                        file_no: p.seg_file,
                    });
                    stacks.push(st);
                }
            }
            Payload::Commit(c) => {
                if let Some((f, _, _)) = c.cs_ref {
                    changesets.push(FileName {
                        family: 9,
                        ref_id: 0,
                        file_no: f,
                    });
                }
            }
            _ => {}
        }
    }
    stacks.sort();
    stacks.dedup();
    let get = |n: &FileName| {
        segs.get(n)
            .map(|(p, s)| (p.as_str(), s))
            .ok_or_else(|| format!("{dir}/{} is named by the store but not a fixture", n.name()))
    };
    let mut failures = Vec::new();
    let mut checked = BTreeSet::new();
    for st in &stacks {
        let layers = st.iter().map(get).collect::<Result<Vec<_>, _>>()?;
        checked.extend(st.iter().copied());
        if let Err(e) = segment::check_stack(&layers, &syms) {
            failures.push(e.to_string());
        }
    }
    for n in &changesets {
        let (p, s) = get(n)?;
        checked.insert(*n);
        if let Err(e) = segment::check_changeset(p, s, &syms) {
            failures.push(e.to_string());
        }
    }
    for (n, (p, _)) in &segs {
        if !checked.contains(n) {
            failures.push(format!("{p}: in no set, promotion or cs_ref of the store"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// [F09 §17.1] V-12 and the set-wide name order: stores A and B.
#[test]
fn hex_store_views() {
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
        if let Err(e) = check_views(dir, head, log, more) {
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
