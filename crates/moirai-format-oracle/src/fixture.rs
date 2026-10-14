//! The fixture checks of E3 (PLAN WP-95): every hand-written hex fixture decodes into typed values and re-encodes
//! byte-identically, and the `moi/` and `carrier/` fixtures' expectation files are read in their framing.
//!
//! - A whole file with a magic ([F03 §4.1] `MLCK`, [F04 §3.1] `MOIR`, [F09 §2.1] `MSEG` with its `seg_kind`,
//!   [F10 §6.1] `MDIC`, [F10 §7.2] `MGMP`) is checked by [`check_file`], with its name against its header when given
//!   ([F09 §17.1] V-3, [`sealed_identity`], [`FileName`]); a `HEAD` by [`check_head`], which also gives the slot
//!   selection of [F04 §8.1].
//! - A log is checked with the `HEAD` of its store by [`check_log`]: the scan of [F05 §5] over its extents, every valid
//!   group re-encoded with its chain trailer; [`check_head_fold`] compares the slot with the [F05 §10.2] fold of the
//!   groups it covers, and [`walk_tail`] decodes the records after the end of the scan on their own.
//! - A base or delta segment's `DROPPED` and `HARVEST` sections ([F11 §13.4], §13.5) are checked against the [F05 §10.3]
//!   fold of the log records of its window over the view of the layers under it ([`TableView`]) by
//!   [`check_segment_folds`] and [`check_table_folds`].
//! - What a sealed file states about itself for the references that name it ([F09 §17.1] V-8) is [`Sealed`].
//! - A fragment, a sequence of one structure to the end of the file, by [`check_fragment`].
//! - The `.expect` and `case.txt` files by [`parse_framed`] and [`hex_block`] (the framing of
//!   `fixtures/canonical/INDEX.md` §2.1 and §2.3).

use std::collections::{BTreeMap, BTreeSet};

use crate::head::{HeadFile, HeadSlot, SlotClass};
use crate::lock::LockFile;
use crate::log::{self, Payload, Scan};
use crate::prim::{Error, Reader, Result, Writer, err, hex};
use crate::runtime::{self, BindingExt, FsTime, OsFileId, Table, VolumeCaps};
use crate::value::{AnchorRec, Item, Value, decode_field_block, encode_field_block};
use crate::{canon, commit, sealed, segment};

/// A whole-file structure, told by its magic bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The `LOCK` file (\[F03\]).
    Lock,
    /// The `HEAD` file (\[F04\]).
    Head,
    /// A graph segment (`seg_kind` 3, 4, 5, 9; \[F09\]).
    Segment,
    /// A `hist` file ([F10 §4]).
    Hist,
    /// A `blobs` file ([F10 §5]).
    Blobs,
    /// A `dict` file ([F10 §6]).
    Dict,
    /// A `gitmap` page ([F10 §7]).
    Gitmap,
}

/// The structure a file's magic names, if any.
pub fn kind_of(b: &[u8]) -> Option<Kind> {
    match b.get(..4)? {
        b"MLCK" => Some(Kind::Lock),
        b"MOIR" => Some(Kind::Head),
        b"MSEG" => Some(match b.get(6) {
            Some(2) => Kind::Hist,
            Some(6) => Kind::Blobs,
            _ => Kind::Segment,
        }),
        b"MDIC" => Some(Kind::Dict),
        b"MGMP" => Some(Kind::Gitmap),
        // [F04 §8.1]: slot A may be absent (never written or torn); slot B then carries the magic.
        _ if b.len() == crate::head::HEAD_LEN && b.get(4096..4100) == Some(b"MOIR") => {
            Some(Kind::Head)
        }
        _ => None,
    }
}

/// Fails unless `got` equals `want`, naming the first differing byte.
pub fn same(what: &str, got: &[u8], want: &[u8]) -> Result<()> {
    if got == want {
        return Ok(());
    }
    let at = got
        .iter()
        .zip(want)
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    err(
        at,
        format!(
            "{what}: the re-encoding differs from the input (lengths {} and {})",
            got.len(),
            want.len()
        ),
    )
}

/// Decodes a whole file by its magic and checks the byte-identical re-encode (E3). A `HEAD` passes when every slot
/// that can be re-encoded is ([`check_head`]); whether a slot can be used is [`HeadFile::choose`]'s. With the file's
/// store name, a sealed file's header is also checked against it ([F09 §17.1] V-3, [`sealed_identity`]).
pub fn check_file(b: &[u8], name: Option<&str>) -> Result<Kind> {
    let Some(k) = kind_of(b) else {
        return err(
            0,
            "no magic of a whole-file structure [F03 §4.1, F04 §3.1, F09 §2.1, F10]",
        );
    };
    match k {
        Kind::Lock => same("LOCK", &LockFile::decode(b)?.encode(), b)?,
        Kind::Head => {
            check_head(b)?;
        }
        Kind::Segment => same(
            "segment",
            &segment::encode_segment(&segment::decode_segment(b)?),
            b,
        )?,
        Kind::Hist => same("hist", &sealed::encode_hist(&sealed::decode_hist(b)?), b)?,
        Kind::Blobs => same("blobs", &sealed::encode_blobs(&sealed::decode_blobs(b)?), b)?,
        Kind::Dict => same("dict", &sealed::encode_dict(&sealed::decode_dict(b)?), b)?,
        Kind::Gitmap => same(
            "gitmap",
            &sealed::encode_gitmap(&sealed::decode_gitmap(b)?),
            b,
        )?,
    }
    if let Some(n) = name
        && !matches!(k, Kind::Lock | Kind::Head)
    {
        sealed_identity(n, b)?;
    }
    Ok(k)
}

/// Decodes a `HEAD` ([F04 §7]) and re-encodes each slot: a valid slot, and a fatal one that kept its fields (`format` 1,
/// reserved bytes zero), from its fields; an absent one as read. A fatal slot of another format, or with a set
/// reserved byte, has nothing to re-encode beyond the fault. The caller asks [`HeadFile::choose`] for the selection of
/// [F04 §8.1].
pub fn check_head(b: &[u8]) -> Result<HeadFile> {
    let h = HeadFile::decode(b)?;
    for (i, s) in [&h.a, &h.b].into_iter().enumerate() {
        let at = i * crate::head::SLOT_LEN;
        let want = &b[at..at + crate::head::SLOT_LEN];
        match s {
            SlotClass::Valid(slot) | SlotClass::Fatal(_, Some(slot)) => {
                same("HEAD slot", &slot.encode(), want).map_err(|e| e.shifted(at))?
            }
            SlotClass::Absent(raw) => same("HEAD slot", raw, want)?,
            SlotClass::Fatal(_, None) => {}
        }
    }
    Ok(h)
}

/// What [`check_log`] found: the scan of [F05 §5] and whether every byte after the end of the valid log is zero.
#[derive(Clone, Debug)]
pub struct LogCheck {
    /// The scan: its valid groups, the end of the valid log and why it stopped.
    pub scan: Scan,
    /// True when every byte of the extents after `scan.end` is zero ([F05 §2.4]).
    pub zero_tail: bool,
}

/// Scans a store's extents (`(n, bytes of log.<n>)`) with the slot `head` selects ([F05 §5.1]), and re-encodes every
/// valid group byte for byte with its chain trailer ([F05 §4.2]).
///
/// The scan starts at `epoch_lsn` with XXH3-64(epoch) when that extent is present, otherwise at the first byte of the
/// first extent given, with the `chain_in` of the `ExtentHead` there ([F05 §4.5]). Symbols are known completely from
/// lsn 0 (a store's first byte); a scan that starts elsewhere has no `SYMTAB` here and checks SD-3 only against the
/// definitions it meets.
pub fn check_log(extents: &[(u32, &[u8])], head: &HeadSlot) -> Result<LogCheck> {
    let e = head.init.log_extent_bytes;
    let Some(&(n0, first)) = extents.iter().min_by_key(|x| x.0) else {
        return err(0, "no extent to scan");
    };
    let ext = |n: u32| extents.iter().find(|x| x.0 == n).map(|x| x.1);
    let epoch_ext = u32::try_from(head.epoch_lsn / e + 1).unwrap_or(u32::MAX);
    let (start, chain) = if ext(epoch_ext).is_some() {
        (head.epoch_lsn, log::epoch_seed(head.epoch))
    } else {
        let (rec, _) = log::Record::decode_detached(first, 0).map_err(|x| match x {
            log::RecError::Invalid(e) | log::RecError::Malformed(e) => e,
        })?;
        let Payload::ExtentHead(h) = &rec.payload else {
            return err(
                0,
                "the first extent does not open with an ExtentHead [F05 §4.5]",
            );
        };
        (u64::from(n0 - 1) * e, h.chain_in)
    };
    let sc = log::ScanCtx {
        ctx: log::Ctx {
            e,
            epoch: head.epoch,
        },
        epoch_lsn: head.epoch_lsn,
        durable_lsn: head.durable_lsn,
        init: head.init,
        project_oid_algo: head.project_oid_algo,
    };
    let mut syms = if start == 0 {
        log::Symbols::from_symtab(&[])
    } else {
        log::Symbols::default()
    };
    let scan = log::scan(ext, &sc, start, chain, &mut syms);
    let mut seed = chain;
    for g in &scan.groups {
        let seed_g = if g.lsn == head.epoch_lsn {
            log::epoch_seed(head.epoch)
        } else {
            seed
        };
        let enc = log::encode_group(g, seed_g);
        let n = u32::try_from(g.lsn / e + 1).unwrap_or(u32::MAX);
        let o = (g.lsn % e) as usize;
        let file = ext(n).expect("a scanned group lies in a given extent");
        same("group", &enc, &file[o..o + enc.len()]).map_err(|x| Error {
            offset: o + x.offset,
            reason: format!("log.{n}: {}", x.reason),
            rule: x.rule,
        })?;
        seed = u64::from_le_bytes(enc[enc.len() - 8..].try_into().expect("8"));
    }
    let end_n = u32::try_from(scan.end / e + 1).unwrap_or(u32::MAX);
    let end_o = (scan.end % e) as usize;
    let zero_tail = extents.iter().all(|&(n, b)| {
        n < end_n
            || b[if n == end_n { end_o } else { 0 }..]
                .iter()
                .all(|&x| x == 0)
    });
    Ok(LogCheck { scan, zero_tail })
}

/// A structure that a fragment fixture holds back to back to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fragment {
    /// Stored op values: a type byte and the value bytes ([F06 §5.1], [F08 §5.1]–§5.2); `absent` admitted.
    OpValues,
    /// Field blocks ([F08 §6.1]).
    FieldBlocks,
    /// Canonical typed values `cv` ([F07 §7.1]).
    CanonicalValues,
    /// Anchor records ([F08 §10.3]).
    AnchorRecords,
    /// Schema item records ([F08 §8.5]) in item key order, as a view's `SCHEMA` section holds them ([F09 §8.3]).
    SchemaItems {
        /// The fragment's symbols of class `name`: ids 1, 2, … dense in first-use order (a fragment carries no `SYMTAB`).
        names: Vec<String>,
    },
    /// Op frames ([F06 §7.1]) as if in a `Commit` record at `lsn`: every `prev` lies below it, and every `Schema` op's
    /// `item_key` is its items' stored key form ([F06 §7.6]).
    OpFrames {
        /// The record's lsn.
        lsn: u64,
        /// The fragment's symbols of class `name`: ids 1, 2, … dense in first-use order (a fragment carries no `SYMTAB`).
        names: Vec<String>,
    },
    /// The given numbers of `OsFileId` ([F11 §12.1]), then `VolumeCaps` (§12.3), then `FsTime` (§12.2).
    OsIds([usize; 3]),
    /// `BindingExt` values ([F18 §3.2]); each must be valid.
    BindingExts,
    /// One `HEADS` row image of a log record ([F11 §5], §2.9).
    HeadsRow,
    /// One `ClientHead` payload ([F05 §9.3]) whose symbol references are among `defined` (class, id).
    ClientHeadPayload {
        /// The symbols the fragment assumes defined.
        defined: Vec<(u8, u32)>,
    },
}

/// A fragment's own symbols of class `name` ([F09 §14.1]), which its first comment names where each is used: ids 1, 2,
/// … dense in first-use order (a fragment carries no `SYMTAB`). They let a fragment check what needs names: the stored
/// key form of a schema item ([F08 §8.5] "Stored key form") and the item key order (§8.5 "Item key order").
struct FragmentNames<'a>(&'a [String]);

impl<'a> FragmentNames<'a> {
    /// The text of symbol `id`.
    fn text(&self, id: u32) -> Option<&'a str> {
        let i = usize::try_from(id).ok()?.checked_sub(1)?;
        self.0.get(i).map(String::as_str)
    }

    /// The item's stored key form; an item naming a symbol the fragment does not define is refused, so no key goes
    /// unchecked.
    fn stored_key(&self, it: &Item, at: usize) -> Result<Vec<u8>> {
        match it.stored_key(|id| self.text(id)) {
            Some(k) => Ok(k),
            None => err(
                at,
                "a schema item names a symbol of class name that the fragment does not define [F08 §8.5]",
            ),
        }
    }
}

/// Decodes a fragment to its end, re-encodes every item byte-identically, and returns the number of items.
pub fn check_fragment(f: &Fragment, b: &[u8]) -> Result<usize> {
    let mut r = Reader::new(b);
    let mut n = 0usize;
    let mut w = Writer::new();
    match f {
        Fragment::OpValues => {
            while !r.is_empty() {
                Value::decode(&mut r, true)?.encode(&mut w);
                n += 1;
            }
        }
        Fragment::FieldBlocks => {
            while !r.is_empty() {
                encode_field_block(&decode_field_block(&mut r)?, &mut w);
                n += 1;
            }
        }
        Fragment::CanonicalValues => {
            while !r.is_empty() {
                w.bytes(&canon::Cv::decode(&mut r)?.encode());
                n += 1;
            }
        }
        Fragment::AnchorRecords => {
            while !r.is_empty() {
                AnchorRec::decode(&mut r)?.encode(&mut w);
                n += 1;
            }
        }
        Fragment::SchemaItems { names } => {
            let names = FragmentNames(names);
            let mut last: Option<(u8, Vec<u8>)> = None;
            while !r.is_empty() {
                let at = r.offset();
                let it = Item::decode(&mut r)?;
                let key = (it.class(), names.stored_key(&it, at)?);
                if last.as_ref().is_some_and(|l| *l >= key) {
                    return err(
                        at,
                        "schema items not strictly ascending in item key order [F08 §8.5, F09 §8.3]",
                    );
                }
                it.encode(&mut w);
                last = Some(key);
                n += 1;
            }
        }
        Fragment::OpFrames { lsn, names } => {
            let names = FragmentNames(names);
            while !r.is_empty() {
                let at = r.offset();
                let op = commit::Op::decode(&mut r)?;
                if op.prev().is_some_and(|p| p >= *lsn) {
                    return err(
                        at,
                        format!("an op's prev is not below its record's lsn {lsn} [F06 §7.3]"),
                    );
                }
                if let commit::Op::Schema { old, new, .. } = &op {
                    for it in old.iter().chain(new.iter()) {
                        names.stored_key(it, at)?;
                    }
                    op.check_schema_key(|id| names.text(id), at)?;
                }
                op.encode(&mut w);
                n += 1;
            }
        }
        Fragment::OsIds([a, v, t]) => {
            for _ in 0..*a {
                OsFileId::decode(&mut r)?.encode(&mut w);
            }
            for _ in 0..*v {
                VolumeCaps::decode(&mut r)?.encode(&mut w);
            }
            for _ in 0..*t {
                FsTime::decode(&mut r)?.encode(&mut w);
            }
            r.finish("the OsFileId, VolumeCaps and FsTime values")?;
            n = a + v + t;
        }
        Fragment::BindingExts => {
            while !r.is_empty() {
                let at = r.offset();
                let x = BindingExt::decode(&mut r)?;
                if !x.valid() {
                    return err(at, "a BindingExt breaks the rules of [F18 §3.2]");
                }
                x.encode(&mut w);
                n += 1;
            }
        }
        Fragment::HeadsRow => {
            let row = runtime::decode_image(Table::Heads, &mut r, runtime::Place::Upsert)?;
            r.finish("a HEADS row image")?;
            runtime::encode_image(&row, &mut w);
            n = 1;
        }
        Fragment::ClientHeadPayload { defined } => {
            let p = Payload::decode(3, &mut r)?;
            r.finish("a ClientHead payload")?;
            if let Some((c, id)) = p
                .symbol_refs()
                .into_iter()
                .find(|&(c, id)| id != 0 && !defined.contains(&(c, id)))
            {
                return err(
                    0,
                    format!("symbol {id} of class {c} is not defined [F05 §8.1 SD-3]"),
                );
            }
            p.encode(&mut w);
            n = 1;
        }
    }
    same("fragment", w.as_slice(), b)?;
    Ok(n)
}

/// One record of a framed file (`fixtures/canonical/INDEX.md` §2.1): its line directives and its blocks, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Framed {
    /// `%% <name> <value>` lines, in order (repeatable ones repeat).
    pub lines: Vec<(String, String)>,
    /// `%% <name>` blocks: the lines up to the next line that starts with `%% `.
    pub blocks: Vec<(String, Vec<String>)>,
}

impl Framed {
    /// The first value of line directive `name`.
    pub fn line(&self, name: &str) -> Option<&str> {
        self.lines
            .iter()
            .find(|l| l.0 == name)
            .map(|l| l.1.as_str())
    }

    /// Every value of line directive `name`, in order.
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|l| l.0 == name)
            .map(|l| l.1.as_str())
            .collect()
    }

    /// The lines of block `name`.
    pub fn block(&self, name: &str) -> Option<&[String]> {
        self.blocks
            .iter()
            .find(|b| b.0 == name)
            .map(|b| b.1.as_slice())
    }
}

/// Parses a framed file into its records: each runs from its first directive to `%% end`; `#` lines outside a block and
/// blank lines outside a record are comments.
pub fn parse_framed(text: &str) -> Result<Vec<Framed>> {
    let mut out = Vec::new();
    let mut cur: Option<Framed> = None;
    let mut block: Option<(String, Vec<String>)> = None;
    for (i, line) in text.split('\n').enumerate() {
        if let Some(d) = line.strip_prefix("%% ") {
            let rec = cur.get_or_insert_with(Framed::default);
            if let Some(b) = block.take() {
                rec.blocks.push(b);
            }
            if d == "end" {
                out.push(cur.take().expect("open record"));
                continue;
            }
            match d.split_once(' ') {
                Some((k, v)) => rec.lines.push((k.to_owned(), v.to_owned())),
                None if !d.is_empty() => block = Some((d.to_owned(), Vec::new())),
                None => return err(i, "an empty directive"),
            }
        } else if let Some(b) = &mut block {
            b.1.push(line.to_owned());
        } else if !(line.is_empty() || line.starts_with('#')) {
            return err(i, format!("line {} is outside every directive", i + 1));
        }
    }
    if cur.is_some() {
        return err(0, "a record without %% end");
    }
    Ok(out)
}

/// The bytes of a hex block (`fixtures/canonical/INDEX.md` §2.3): hexadecimal digit pairs, whitespace ignored, `;` to
/// the end of the line a comment, `<hex> * <n>` n copies.
pub fn hex_block(lines: &[String]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let body = line.split(';').next().unwrap_or("");
        let (digits, times) = match body.split_once('*') {
            Some((d, n)) => match n.trim().parse::<usize>() {
                Ok(n) => (d, n),
                Err(_) => return err(i, "a repetition count is not a decimal"),
            },
            None => (body, 1),
        };
        let d: String = digits.chars().filter(|c| !c.is_whitespace()).collect();
        let Some(b) = crate::prim::unhex(&d) else {
            return err(i, format!("line {} of a hex block is not hex pairs", i + 1));
        };
        for _ in 0..times {
            out.extend_from_slice(&b);
        }
    }
    Ok(out)
}

/// A store file name of [F02 §6.3] that names a sealed file: its family (the [F11 §2.5] `FileFamily` value, which is
/// also the `seg_kind` of a segment family), the `ref_id` of a `seg.b<ref_id>.<K>`, and its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName {
    /// 2 `hist`, 3 `seg.base`, 4 `seg.d`, 5 `seg.b`, 6 `blobs`, 7 `dict`, 8 `gitmap`, 9 `cs`.
    pub family: u8,
    /// The ref id of family 5; 0 otherwise.
    pub ref_id: u32,
    /// The file number, ≥ 1.
    pub file_no: u32,
}

/// `fnum` of [F02 §6.3]: decimal, no leading zero, 1 … 2^32 − 1.
fn fnum(s: &str) -> Option<u32> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl FileName {
    /// Parses `hist.<n>`, `seg.base.<n>`, `seg.d<n>`, `seg.b<ref_id>.<n>`, `blobs.<n>`, `dict.<n>`, `gitmap.<n>` or
    /// `cs.<n>` ([F02 §6.3]); `None` for any other name.
    pub fn parse(name: &str) -> Option<FileName> {
        let plain = |family: u8, rest: &str| {
            fnum(rest).map(|file_no| FileName {
                family,
                ref_id: 0,
                file_no,
            })
        };
        if let Some(r) = name.strip_prefix("seg.base.") {
            plain(3, r)
        } else if let Some(r) = name.strip_prefix("seg.d") {
            plain(4, r)
        } else if let Some(r) = name.strip_prefix("seg.b") {
            let (id, k) = r.split_once('.')?;
            let ref_id = if id == "0" { 0 } else { fnum(id)? };
            Some(FileName {
                family: 5,
                ref_id,
                file_no: fnum(k)?,
            })
        } else {
            let (word, r) = name.split_once('.')?;
            let family = match word {
                "hist" => 2,
                "blobs" => 6,
                "dict" => 7,
                "gitmap" => 8,
                "cs" => 9,
                _ => return None,
            };
            plain(family, r)
        }
    }

    /// The name of the file ([F02 §6.3]).
    pub fn name(&self) -> String {
        let n = self.file_no;
        match self.family {
            2 => format!("hist.{n}"),
            3 => format!("seg.base.{n}"),
            4 => format!("seg.d{n}"),
            5 => format!("seg.b{}.{n}", self.ref_id),
            6 => format!("blobs.{n}"),
            7 => format!("dict.{n}"),
            8 => format!("gitmap.{n}"),
            _ => format!("cs.{n}"),
        }
    }
}

/// What a sealed file states about itself, for the references that name it ([F09 §17.1] V-8): `total_len`, the 16-byte
/// digest a reference carries ([F09 §14.4] `digest16`: `seg_digest[0..16]` of a `SegHdr` file, the header `digest` of
/// a `dict` or `gitmap`), and the `SegHdr` bounds (0 where the family has none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sealed {
    /// The file's name parts.
    pub name: FileName,
    /// `total_len`.
    pub total_len: u64,
    /// `digest16`.
    pub digest16: [u8; 16],
    /// `SegHdr.from_lsn`.
    pub from_lsn: u64,
    /// `SegHdr.upto_lsn`.
    pub upto_lsn: u64,
}

/// [F09 §17.1] V-3 and [F10 §2] self-identification: reads a sealed file's header and checks it against the file's
/// name (the family's magic and `seg_kind`, `file_no`, and a `seg-branch`'s `ref_id`; a formatted dictionary's
/// `Dictionary_ID`, [F10 §6.2], [`sealed::check_dict_number`]).
pub fn sealed_identity(name: &str, b: &[u8]) -> Result<Sealed> {
    let Some(n) = FileName::parse(name) else {
        return err(0, format!("{name:?} is not a sealed file name [F02 §6.3]"));
    };
    let mut digest16 = [0u8; 16];
    match b.get(..4) {
        Some(b"MSEG") => {
            let c = segment::decode_container(b)?;
            let h = &c.hdr;
            if h.seg_kind != n.family || h.file_no != n.file_no || h.ref_id != n.ref_id {
                return err(
                    6,
                    format!(
                        "SegHdr seg_kind {}, file_no {}, ref_id {} do not match the name {name} [F09 §17.1 V-3]",
                        h.seg_kind, h.file_no, h.ref_id
                    ),
                );
            }
            digest16.copy_from_slice(&h.seg_digest[..16]);
            Ok(Sealed {
                name: n,
                total_len: h.total_len,
                digest16,
                from_lsn: h.from_lsn,
                upto_lsn: h.upto_lsn,
            })
        }
        Some(b"MDIC") if n.family == 7 => {
            let d = sealed::decode_dict(b)?;
            sealed::check_dict_number(&d, n.file_no)?;
            Ok(Sealed {
                name: n,
                total_len: b.len() as u64,
                digest16: d.digest,
                from_lsn: 0,
                upto_lsn: 0,
            })
        }
        Some(b"MGMP") if n.family == 8 => {
            let g = sealed::decode_gitmap(b)?;
            if g.file_no != n.file_no {
                return err(
                    12,
                    format!(
                        "GitmapHdr file_no {} does not match the name {name} [F10 §7.2]",
                        g.file_no
                    ),
                );
            }
            digest16.copy_from_slice(&b[24..40]);
            Ok(Sealed {
                name: n,
                total_len: b.len() as u64,
                digest16,
                from_lsn: 0,
                upto_lsn: 0,
            })
        }
        _ => err(
            0,
            format!("{name}: the magic is not its name family's [F09 §17.1 V-3]"),
        ),
    }
}

/// The log-derived fields of a `HEAD` slot ([F04 §6]) as the [F05 §10.2] fold of a run of groups makes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fold {
    commit_seq: u64,
    next_id: u32,
    next_anchor: u32,
    fence: u64,
    next_file_no: u32,
    next_ref_id: u32,
    hlc_seq: u64,
    hlc_commit: u64,
    seq_ring: [Option<(u64, u64)>; 32],
    refs_lsn: Option<u64>,
    pins_lsn: Option<u64>,
    heads_lsn: Option<u64>,
    markers_lsn: Option<u64>,
    image_cursor: [(u8, u8, u64); 4],
    set: Option<(u64, u32, Vec<crate::head::SegRef>)>,
    fts_tier2: bool,
}

impl Fold {
    fn file(&mut self, n: u32) {
        self.next_file_no = self.next_file_no.max(n.saturating_add(1));
    }

    /// [F04 §5.11]: a covered `GitMap` for (`dest`, `algo`) with `cursor_seq` s.
    fn cursor(&mut self, dest: u8, algo: u8, s: u64) {
        let c = &mut self.image_cursor;
        if let Some(e) = c.iter_mut().find(|e| e.0 == dest && e.1 == algo) {
            e.2 = e.2.max(s);
        } else if let Some(e) = c.iter_mut().find(|e| e.0 == 0) {
            *e = (dest, algo, s);
        } else {
            let i = (0..4).min_by_key(|&i| (c[i].2, i)).expect("four entries");
            c[i] = (dest, algo, s);
        }
    }

    /// One record's effect ([F05 §10.2]).
    fn apply(&mut self, lsn: u64, p: &Payload) {
        use crate::commit::Op;
        match p {
            Payload::Commit(c) => {
                self.commit_seq = self.commit_seq.max(c.seq);
                for op in &c.ops {
                    if let Op::Create { id, .. } | Op::CreateDeleted { id, .. } = op {
                        self.next_id = self.next_id.max(id.saturating_add(1));
                    }
                    if let Some(a) = op.anchor_no() {
                        self.next_anchor = self.next_anchor.max(a.saturating_add(1));
                    }
                }
                self.seq_ring[(c.seq % 32) as usize] = Some((c.seq, lsn));
                if let Some((f, _, _)) = c.cs_ref {
                    self.file(f);
                }
                self.hlc_seq = self.hlc_seq.max(c.append_hlc());
                self.hlc_commit = self.hlc_commit.max(c.hlc);
            }
            Payload::RefUpdate(u) => {
                if matches!(u.reason, 1 | 5) {
                    self.next_ref_id = self.next_ref_id.max(u.ref_id.saturating_add(1));
                }
                self.hlc_seq = self.hlc_seq.max(u.hlc);
            }
            Payload::ClientHead(h) => {
                self.heads_lsn = Some(lsn);
                let hlc = match h {
                    log::ClientHead::Set(row) => row.u("hlc"),
                    log::ClientHead::Remove(_, _, hlc) => *hlc,
                };
                self.hlc_seq = self.hlc_seq.max(hlc);
            }
            Payload::Lease(l) => {
                self.fence = self.fence.max(l.token);
                self.hlc_seq = self.hlc_seq.max(l.hlc);
            }
            Payload::Marker(es) => {
                self.markers_lsn = Some(lsn);
                for e in es {
                    self.hlc_seq = self.hlc_seq.max(e.hlc);
                }
            }
            Payload::Idem(i) => self.hlc_seq = self.hlc_seq.max(i.append_hlc),
            Payload::Backup(_, _, _, hlc)
            | Payload::FsIntentDone(_, _, hlc, _)
            | Payload::FsIntentAborted(_, _, _, hlc) => self.hlc_seq = self.hlc_seq.max(*hlc),
            Payload::FsIntent(f) => self.hlc_seq = self.hlc_seq.max(f.hlc),
            Payload::GitMap(g) => self.cursor(g.dest, g.algo.byte(), g.cursor_seq),
            Payload::Pin(_) => self.pins_lsn = Some(lsn),
            Payload::Checkpoint(c) => {
                if let Some(set) = &c.set {
                    set.2.iter().for_each(|s| self.file(s.file_no));
                    self.set = Some(set.clone());
                }
                if c.ckflags & (1 << 7) != 0 {
                    self.fts_tier2 = true;
                }
                self.next_file_no = self.next_file_no.max(c.next_file_no);
                for p in c.promotions.iter().flatten() {
                    self.file(p.seg_file);
                }
                for r in c.retirements.iter().flatten() {
                    self.file(r.hist_file);
                }
                for e in c.added.iter().flatten() {
                    self.file(e.file.file_no);
                }
                for f in c.released.iter().flatten() {
                    self.file(f.file_no);
                }
            }
            Payload::RefTable(es) => {
                self.refs_lsn = Some(lsn);
                for e in es.iter().filter(|e| e.upsert.is_some()) {
                    self.next_ref_id = self.next_ref_id.max(e.ref_id.saturating_add(1));
                }
            }
            Payload::Reserve(r) => {
                self.next_id = self.next_id.max(r.first_id.saturating_add(r.n_ids));
                self.next_anchor = self
                    .next_anchor
                    .max(r.first_anchor.saturating_add(r.n_anchors));
                self.file(r.cs_file);
                if r.blobs_file != 0 {
                    self.file(r.blobs_file);
                }
            }
            Payload::ExtentHead(h) => {
                self.commit_seq = self.commit_seq.max(h.commit_seq);
                self.next_id = self.next_id.max(h.next_id);
                self.next_anchor = self.next_anchor.max(h.next_anchor);
                self.fence = self.fence.max(h.fence);
                self.next_file_no = self.next_file_no.max(h.next_file_no);
                self.next_ref_id = self.next_ref_id.max(h.next_ref_id);
                self.hlc_seq = self.hlc_seq.max(h.hlc_seq);
                self.hlc_commit = self.hlc_commit.max(h.hlc_commit);
            }
            // [F05 §7] rows 29 and 30, §10.2: both raise `hlc_seq`.
            Payload::BodyDrop(d) => self.hlc_seq = self.hlc_seq.max(d.hlc),
            Payload::Harvest(h) => self.hlc_seq = self.hlc_seq.max(h.hlc),
            _ => {}
        }
    }
}

/// [F04 §6], [F05 §10.2]: the log-derived fields of `slot` against the fold of the scanned groups it covers (those
/// ending at or below `committed_lsn`). A scan starts at an extent head, which carries the counters and HLC maxima as
/// they stood before it (§9.28), so these are compared for equality. The table pointers, `image_cursor`, `seq_ring`,
/// the segment set and `fts_tier2` are compared where a covered record sets them, and wholly when the scan starts at
/// lsn 0 (nothing precedes it). A scan that stopped at corruption below `committed_lsn` covers a prefix of what the slot
/// folded: its counters may only be at most the slot's.
pub fn check_head_fold(slot: &HeadSlot, scan: &Scan) -> Result<()> {
    let Some(start) = scan.groups.first().map(|g| g.lsn) else {
        return Ok(());
    };
    let mut f = Fold::default();
    for g in scan
        .groups
        .iter()
        .filter(|g| g.lsn + g.len <= slot.committed_lsn)
    {
        for r in &g.records {
            f.apply(r.hdr.lsn, &r.payload);
        }
    }
    let prefix = matches!(scan.stop, log::Stop::Corrupt(_)) && scan.end < slot.committed_lsn;
    let fail = |what: &str, got: String, want: String| {
        err(
            0,
            format!(
                "HEAD {what} is {want}, the fold of the log it covers gives {got} [F04 §6, F05 §10.2]"
            ),
        )
    };
    let counters = [
        ("commit_seq", f.commit_seq, slot.commit_seq),
        ("next_id", u64::from(f.next_id), u64::from(slot.next_id)),
        (
            "next_anchor",
            u64::from(f.next_anchor),
            u64::from(slot.next_anchor),
        ),
        ("fence", f.fence, slot.fence),
        (
            "next_file_no",
            u64::from(f.next_file_no),
            u64::from(slot.next_file_no),
        ),
        (
            "next_ref_id",
            u64::from(f.next_ref_id),
            u64::from(slot.next_ref_id),
        ),
        ("hlc_seq", f.hlc_seq, slot.hlc_seq),
        ("hlc_commit", f.hlc_commit, slot.hlc_commit),
    ];
    for (what, got, want) in counters {
        if if prefix { got > want } else { got != want } {
            return fail(what, got.to_string(), want.to_string());
        }
    }
    if prefix {
        return Ok(());
    }
    let from_zero = start == 0;
    let pointers = [
        ("refs_lsn", f.refs_lsn, slot.refs_lsn),
        ("pins_lsn", f.pins_lsn, slot.pins_lsn),
        ("heads_lsn", f.heads_lsn, slot.heads_lsn),
        ("markers_lsn", f.markers_lsn, slot.markers_lsn),
    ];
    for (what, got, want) in pointers {
        let ok = match got {
            Some(l) => l == want,
            None if from_zero => want == 0,
            None => want < start,
        };
        if !ok {
            return fail(what, format!("{got:?}"), want.to_string());
        }
    }
    for (k, e) in f.seq_ring.iter().enumerate() {
        let have = slot.seq_ring[k];
        let ok = match e {
            Some((s, l)) => have.seq == *s && have.lsn == *l,
            None => !from_zero || (have.seq == 0 && have.lsn == 0),
        };
        if !ok {
            return fail(
                &format!("seq_ring[{k}]"),
                format!("{e:?}"),
                format!("({}, {})", have.seq, have.lsn),
            );
        }
    }
    if from_zero {
        let slot_cursor: Vec<(u8, u8, u64)> = slot
            .image_cursor
            .iter()
            .map(|c| (c.dest, c.algo, c.seq))
            .collect();
        if slot_cursor != f.image_cursor {
            return fail(
                "image_cursor",
                format!("{:?}", f.image_cursor),
                format!("{slot_cursor:?}"),
            );
        }
    }
    let tier2 = slot.flags & 2 != 0;
    if (f.fts_tier2 && !tier2) || (from_zero && f.fts_tier2 != tier2) {
        return fail(
            "flags.fts_tier2",
            f.fts_tier2.to_string(),
            tier2.to_string(),
        );
    }
    let n = usize::from(slot.n_segments);
    match &f.set {
        Some((upto, active_log, segs)) => {
            if slot.checkpoint_lsn != *upto
                || slot.active_log != *active_log
                || slot.segments[..n] != segs[..]
            {
                return fail(
                    "segment set (checkpoint_lsn, active_log, segments)",
                    format!("({upto}, {active_log}, {segs:?})"),
                    format!(
                        "({}, {}, {:?})",
                        slot.checkpoint_lsn,
                        slot.active_log,
                        &slot.segments[..n]
                    ),
                );
            }
        }
        None if from_zero && n != 0 => {
            return fail("segments", "none".into(), format!("{n} entries"));
        }
        None => {}
    }
    Ok(())
}

/// The records of a log range `[start, end)` in log order, as (record lsn, payload): the input of the table folds
/// ([F05 §10.1]: groups in log order, records in group order).
#[derive(Clone, Debug)]
pub struct FoldRecords<'a> {
    /// The first lsn the records cover.
    pub start: u64,
    /// The end of the range they cover.
    pub end: u64,
    /// The records, in log order.
    pub records: Vec<(u64, &'a Payload)>,
}

impl<'a> FoldRecords<'a> {
    /// Every record of every valid group of a scan: from the first group's lsn (or the scan's end, with no group) to the
    /// end of the valid log.
    pub fn of_scan(scan: &'a Scan) -> FoldRecords<'a> {
        FoldRecords {
            start: scan.groups.first().map_or(scan.end, |g| g.lsn),
            end: scan.end,
            records: scan
                .groups
                .iter()
                .flat_map(|g| g.records.iter().map(|r| (r.hdr.lsn, &r.payload)))
                .collect(),
        }
    }
}

/// A `DROPPED` row's value ([F11 §13.4]), keyed by its `hash`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DroppedRow {
    /// The lsn of the `BodyDrop` record that dropped the hash first.
    pub drop_lsn: u64,
    /// That record's `hlc`.
    pub hlc: u64,
    /// Its `reason`.
    pub reason: u8,
}

/// A live `HARVEST` row's value ([F11 §13.5]), keyed by its `source`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HarvestRow {
    /// `head`.
    pub head: [u8; 16],
    /// `seen`.
    pub seen: u64,
    /// `path`.
    pub path: String,
    /// `ranges`, in the order added.
    pub ranges: Vec<runtime::HarvestRange>,
}

/// The `DROPPED` and `HARVEST` tables as of a stored layer of a segment set ([F11 §2.4]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableView {
    /// `DROPPED` by hash.
    pub dropped: BTreeMap<[u8; 16], DroppedRow>,
    /// `HARVEST` by source.
    pub harvest: BTreeMap<String, HarvestRow>,
}

const DROPPED_TAG: u16 = 0x020D;
const HARVEST_TAG: u16 = 0x020E;

/// The kind of a segment that may carry the two tables ([F11 §2.4]: base and delta segments only).
fn table_seg_kind(seg_kind: u8) -> Result<runtime::SegKind> {
    match seg_kind {
        3 => Ok(runtime::SegKind::Base),
        4 => Ok(runtime::SegKind::Delta),
        k => err(
            0,
            format!("seg_kind {k} carries no DROPPED or HARVEST section [F11 §2.4]"),
        ),
    }
}

/// A segment's `DROPPED` and `HARVEST` sections, each `None` when absent ([F11 §2.4] "Conditional sections").
fn table_sections(
    seg: &segment::Segment,
) -> (Option<&runtime::Section>, Option<&runtime::Section>) {
    let get = |tag: u16| {
        seg.sections.iter().find_map(|(e, s)| match s {
            segment::Sec::Runtime(r) if e.tag == tag => Some(&**r),
            _ => None,
        })
    };
    (get(DROPPED_TAG), get(HARVEST_TAG))
}

fn want_table(s: &runtime::Section, tag: u16) -> Result<()> {
    if s.table.tag() != tag {
        return err(
            0,
            format!(
                "a {} section given for tag 0x{tag:04X} [F11 §2.8]",
                s.table.name()
            ),
        );
    }
    Ok(())
}

/// The rows of a `DROPPED` section; an absent one is the empty table ([F11 §2.4]).
fn dropped_rows(s: Option<&runtime::Section>) -> Result<BTreeMap<[u8; 16], DroppedRow>> {
    let mut m = BTreeMap::new();
    if let Some(s) = s {
        want_table(s, DROPPED_TAG)?;
        for r in &s.rows {
            let hash: [u8; 16] = r.bytes("hash").try_into().expect("b16");
            m.insert(
                hash,
                DroppedRow {
                    drop_lsn: r.u("drop_lsn"),
                    hlc: r.u("hlc"),
                    reason: r.u("reason") as u8,
                },
            );
        }
    }
    Ok(m)
}

/// The rows of a `HARVEST` section by source, `None` for a dead row ([F11 §2.4]); an absent one has no row.
fn harvest_rows(s: Option<&runtime::Section>) -> Result<BTreeMap<String, Option<HarvestRow>>> {
    let mut m = BTreeMap::new();
    if let Some(s) = s {
        want_table(s, HARVEST_TAG)?;
        for r in &s.rows {
            let text = |n: &str| match r.slice(n) {
                runtime::Slice::Text(t) => t.clone(),
                other => panic!("HARVEST {n} is not text: {other:?}"),
            };
            let row = (r.u("flags") & 0x80 == 0).then(|| HarvestRow {
                head: r.bytes("head").try_into().expect("b16"),
                seen: r.u("seen"),
                path: text("path"),
                ranges: match r.slice("ranges") {
                    runtime::Slice::HarvestRanges(v) => v.clone(),
                    other => panic!("HARVEST ranges is not a range list: {other:?}"),
                },
            });
            m.insert(text("source"), row);
        }
    }
    Ok(m)
}

impl TableView {
    /// Adds one stored layer on top ([F11 §2.4]): `DROPPED` is a snapshot in a base and in a delta, and replaces the view
    /// (an absent section is the empty table); `HARVEST` replaces it in a base, and in a delta sets each live row and
    /// removes each dead row's key.
    pub fn push(
        &mut self,
        kind: runtime::SegKind,
        dropped: Option<&runtime::Section>,
        harvest: Option<&runtime::Section>,
    ) -> Result<()> {
        self.dropped = dropped_rows(dropped)?;
        let rows = harvest_rows(harvest)?;
        if kind == runtime::SegKind::Base {
            self.harvest.clear();
        }
        for (source, row) in rows {
            match row {
                Some(r) => {
                    self.harvest.insert(source, r);
                }
                None => {
                    self.harvest.remove(&source);
                }
            }
        }
        Ok(())
    }

    /// [`TableView::push`] for a base or delta segment.
    pub fn push_segment(&mut self, seg: &segment::Segment) -> Result<()> {
        let kind = table_seg_kind(seg.hdr.seg_kind)?;
        let (d, h) = table_sections(seg);
        self.push(kind, d, h)
    }
}

/// [F05 §10.3] into [F11 §13.4] and §13.5: the `DROPPED` and `HARVEST` sections of a base or delta segment whose window is
/// `[from, upto)` ([F09 §2.3]) against the fold of the window's records over `below`, the view of the stored layers
/// under it (unused for a base, whose window starts at 0).
///
/// - `records` must cover the window: `start` ≤ `from` and `upto` ≤ `end`.
/// - `DROPPED` is a snapshot in both kinds: the rows below (none for a base) plus a row per hash of each `BodyDrop` of
///   the window, with the record's lsn as `drop_lsn`, its `hlc` and its `reason`; a hash that has a row keeps it.
/// - `HARVEST` folds each `Harvest` of the window onto the rows below: a mark creates the row or sets `path`, `head` and
///   `seen`; a range appends `{start, end, commit, commit_lsn}`, `commit_lsn` being the lsn of the newest earlier
///   `Commit` record whose `commit_id` begins with `commit`; a forget keeps the ranges whose `start` is below `from`, and
///   with 0 removes the row. A base holds the snapshot; a delta's layer holds a row for every source the window touched,
///   live with its full state, or dead when the source has no row at `upto` (R-70).
/// - An absent section equals an empty expectation (§2.4 "Conditional sections"). The first difference is refused,
///   naming the table, the key, the field and both values.
pub fn check_table_folds(
    kind: runtime::SegKind,
    (from, upto): (u64, u64),
    dropped: Option<&runtime::Section>,
    harvest: Option<&runtime::Section>,
    below: &TableView,
    records: &FoldRecords<'_>,
) -> Result<()> {
    if records.start > from || upto > records.end {
        return err(
            0,
            format!(
                "the records given cover [{}, {}), not the window [{from}, {upto}) [F05 §10.3, F09 §2.3]",
                records.start, records.end
            ),
        );
    }
    let base = kind == runtime::SegKind::Base;
    let in_window = |lsn: u64| from <= lsn && lsn < upto;
    // DROPPED ([F11 §13.4]).
    let mut want_d = if base {
        BTreeMap::new()
    } else {
        below.dropped.clone()
    };
    for (lsn, p) in &records.records {
        if let (true, Payload::BodyDrop(d)) = (in_window(*lsn), p) {
            for h in &d.hashes {
                want_d.entry(*h).or_insert(DroppedRow {
                    drop_lsn: *lsn,
                    hlc: d.hlc,
                    reason: d.reason,
                });
            }
        }
    }
    let got_d = dropped_rows(dropped)?;
    let cite_d = "[F11 §13.4, F05 §10.3]";
    for h in got_d.keys().chain(want_d.keys()).collect::<BTreeSet<_>>() {
        let hx = hex(h);
        match (got_d.get(h), want_d.get(h)) {
            (Some(_), None) => {
                return err(
                    0,
                    format!("DROPPED row {hx}: not given by the fold {cite_d}"),
                );
            }
            (None, Some(w)) => {
                return err(
                    0,
                    format!("DROPPED: no row for {hx}, which the fold gives as {w:?} {cite_d}"),
                );
            }
            (Some(g), Some(w)) => {
                for (f, a, b) in [
                    ("drop_lsn", g.drop_lsn, w.drop_lsn),
                    ("hlc", g.hlc, w.hlc),
                    ("reason", u64::from(g.reason), u64::from(w.reason)),
                ] {
                    if a != b {
                        return err(
                            0,
                            format!("DROPPED row {hx}: {f} {a}, the fold gives {b} {cite_d}"),
                        );
                    }
                }
            }
            (None, None) => {}
        }
    }
    // HARVEST ([F11 §13.5]).
    let cite_h = "[F11 §13.5, F05 §10.3]";
    let mut state = if base {
        BTreeMap::new()
    } else {
        below.harvest.clone()
    };
    let mut touched = BTreeSet::new();
    let mut commits: BTreeMap<[u8; 16], u64> = BTreeMap::new();
    for (lsn, p) in &records.records {
        match p {
            Payload::Commit(c) => {
                commits.insert(c.commit_id[..16].try_into().expect("16"), *lsn);
            }
            Payload::Harvest(h) if in_window(*lsn) => {
                touched.insert(h.source.clone());
                match &h.op {
                    log::HarvestOp::Mark { path, head, seen } => {
                        let row: &mut HarvestRow = state.entry(h.source.clone()).or_default();
                        (row.path, row.head, row.seen) = (path.clone(), *head, *seen);
                    }
                    log::HarvestOp::Range { start, end, commit } => {
                        let Some(row) = state.get_mut(&h.source) else {
                            return err(
                                0,
                                format!(
                                    "HARVEST: the range at lsn {lsn} is for source {:?}, which has no row [F11 §13.5]",
                                    h.source
                                ),
                            );
                        };
                        let Some(&commit_lsn) = commits.get(commit) else {
                            return err(
                                0,
                                format!(
                                    "HARVEST: the range at lsn {lsn} names commit {}, which no earlier Commit record carries [F11 §13.5, F05 §9.30]",
                                    hex(commit)
                                ),
                            );
                        };
                        row.ranges.push(runtime::HarvestRange {
                            start: *start,
                            end: *end,
                            commit: *commit,
                            commit_lsn,
                        });
                    }
                    log::HarvestOp::Forget { from: f } => {
                        if *f == 0 {
                            state.remove(&h.source);
                        } else if let Some(row) = state.get_mut(&h.source) {
                            row.ranges.retain(|r| r.start < *f);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let want_h: BTreeMap<String, Option<HarvestRow>> = if base {
        state.into_iter().map(|(s, r)| (s, Some(r))).collect()
    } else {
        touched
            .into_iter()
            .map(|s| {
                let r = state.get(&s).cloned();
                (s, r)
            })
            .collect()
    };
    let got_h = harvest_rows(harvest)?;
    for s in got_h.keys().chain(want_h.keys()).collect::<BTreeSet<_>>() {
        let live = |r: &Option<HarvestRow>| {
            if r.is_some() {
                "a live row"
            } else {
                "a dead row"
            }
        };
        match (got_h.get(s), want_h.get(s)) {
            (Some(_), None) => {
                return err(
                    0,
                    format!("HARVEST row {s:?}: not given by the fold {cite_h}"),
                );
            }
            (None, Some(w)) => {
                return err(
                    0,
                    format!(
                        "HARVEST: no row for {s:?}, which the fold gives as {} {cite_h}",
                        live(w)
                    ),
                );
            }
            (Some(Some(g)), Some(Some(w))) => {
                let field = if g.head != w.head {
                    Some(("head", hex(&g.head), hex(&w.head)))
                } else if g.seen != w.seen {
                    Some(("seen", g.seen.to_string(), w.seen.to_string()))
                } else if g.path != w.path {
                    Some(("path", format!("{:?}", g.path), format!("{:?}", w.path)))
                } else {
                    None
                };
                if let Some((f, a, b)) = field {
                    return err(
                        0,
                        format!("HARVEST row {s:?}: {f} {a}, the fold gives {b} {cite_h}"),
                    );
                }
                if g.ranges != w.ranges {
                    let same_but_lsn = g.ranges.len() == w.ranges.len()
                        && g.ranges
                            .iter()
                            .zip(&w.ranges)
                            .all(|(a, b)| (a.start, a.end, a.commit) == (b.start, b.end, b.commit));
                    if same_but_lsn {
                        let (i, (a, b)) = g
                            .ranges
                            .iter()
                            .zip(&w.ranges)
                            .enumerate()
                            .find(|(_, (a, b))| a.commit_lsn != b.commit_lsn)
                            .expect("a range differs");
                        return err(
                            0,
                            format!(
                                "HARVEST row {s:?}: ranges[{i}] commit_lsn {}, the fold gives {} {cite_h}",
                                a.commit_lsn, b.commit_lsn
                            ),
                        );
                    }
                    let show = |v: &[runtime::HarvestRange]| {
                        v.iter()
                            .map(|r| {
                                format!(
                                    "({}, {}, {}, {})",
                                    r.start,
                                    r.end,
                                    hex(&r.commit),
                                    r.commit_lsn
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    return err(
                        0,
                        format!(
                            "HARVEST row {s:?}: ranges [{}], the fold gives [{}] {cite_h}",
                            show(&g.ranges),
                            show(&w.ranges)
                        ),
                    );
                }
            }
            (Some(g), Some(w)) if g.is_some() != w.is_some() => {
                return err(
                    0,
                    format!(
                        "HARVEST row {s:?}: {}, the fold gives {} {cite_h}",
                        live(g),
                        live(w)
                    ),
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// [`check_table_folds`] for a base or delta segment: its kind, its window `[from_lsn, upto_lsn)` and its two sections.
pub fn check_segment_folds(
    seg: &segment::Segment,
    below: &TableView,
    records: &FoldRecords<'_>,
) -> Result<()> {
    let kind = table_seg_kind(seg.hdr.seg_kind)?;
    let (d, h) = table_sections(seg);
    check_table_folds(
        kind,
        (seg.hdr.from_lsn, seg.hdr.upto_lsn),
        d,
        h,
        below,
        records,
    )
}

/// One record found after the end of a scan ([F05 §5.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailRecord {
    /// Its offset in its extent.
    pub offset: usize,
    /// Its `len`.
    pub len: usize,
    /// Its kind when it decodes detached ([`log::Record::decode_detached`]; it then re-encodes byte for byte), or why
    /// it does not.
    pub decoded: core::result::Result<u8, String>,
}

/// Walks the bytes of `extent` from offset `from` (the end of a scan) record header by record header: each record is
/// decoded detached and must re-encode byte for byte; one that does not decode is stepped over by its `len`. The walk
/// ends where the rest of the extent is zero, or at a header whose `len` cannot be a record's (below 32 or past the
/// extent). The records found are returned in order.
pub fn walk_tail(extent: &[u8], from: usize) -> Result<Vec<TailRecord>> {
    let mut out = Vec::new();
    let mut o = from;
    // One past the extent's last non-zero byte: the rest of the extent from o is zero exactly when o reaches it.
    let live = extent.iter().rposition(|&x| x != 0).map_or(0, |i| i + 1);
    while o + log::HDR <= extent.len() && o < live {
        let len = u32::from_le_bytes(extent[o..o + 4].try_into().expect("4")) as usize;
        if len < log::HDR || o + len > extent.len() {
            break;
        }
        let decoded = match log::Record::decode_detached(&extent[o..], o) {
            Ok((r, n)) => {
                same(
                    "a record after the end of the scan",
                    &r.encode(),
                    &extent[o..o + n],
                )
                .map_err(|e| e.shifted(o))?;
                Ok(r.hdr.kind)
            }
            Err(log::RecError::Invalid(e) | log::RecError::Malformed(e)) => Err(e.reason),
        };
        out.push(TailRecord {
            offset: o,
            len,
            decoded,
        });
        o += len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dispatch by magic.
    #[test]
    fn dispatch() {
        let mut lock = b"MLCK".to_vec();
        lock.resize(64, 0);
        assert_eq!(kind_of(&lock), Some(Kind::Lock));
        assert_eq!(kind_of(b"MSEG\x01\x00\x06"), Some(Kind::Blobs));
        assert_eq!(kind_of(b"MSEG\x01\x00\x02"), Some(Kind::Hist));
        assert_eq!(kind_of(b"\x28\x00\x00\x00\x0c\x03"), None);
        assert!(check_file(b"xyz", None).is_err());
    }

    /// The framing: line directives, blocks, comments, `%% end`; hex blocks with comments and repetitions.
    #[test]
    fn framing() {
        let t = "# c\n\n%% case a\n%% source [F07 §3]\n%% source [F07 §4]\n%% commit\nkind ordinary\n# kept\n%% c\n; x\n0a 0b ; y\n00 * 3\n%% end\n";
        let r = parse_framed(t).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].line("case"), Some("a"));
        assert_eq!(r[0].all("source"), vec!["[F07 §3]", "[F07 §4]"]);
        assert_eq!(r[0].block("commit").unwrap(), ["kind ordinary", "# kept"]);
        assert_eq!(
            hex_block(r[0].block("c").unwrap()).unwrap(),
            vec![0x0a, 0x0b, 0, 0, 0]
        );
        assert!(parse_framed("%% case a\n").is_err());
        assert!(parse_framed("stray\n").is_err());
        assert!(hex_block(&["0g".to_owned()]).is_err());
    }

    /// [F04 §7], §8.1: two valid slots re-encode and the newer is selected; a torn slot is kept as read and skipped.
    #[test]
    fn head_selection() {
        use crate::head::{Choice, tests::initial_slot};
        let b = [initial_slot(1).encode(), initial_slot(2).encode()].concat();
        let h = check_head(&b).unwrap();
        assert_eq!(h.choose().unwrap().0, Choice::B);
        let mut torn = b.clone();
        torn[4096 + 600] ^= 1;
        assert_eq!(check_head(&torn).unwrap().choose().unwrap().0, Choice::A);
    }

    /// [F05 §5.3]: a two-group extent scans to its end with a zero tail; stale bytes after it are reported; a damaged
    /// group below `durable_lsn` is corruption.
    #[test]
    fn log_scan_verdicts() {
        use crate::head::tests::initial_slot;
        use crate::log::tests::{EPOCH, head, rec};
        let mut ext = vec![0u8; 1 << 16];
        let g0 = log::Group {
            lsn: 0,
            records: vec![head(0, log::epoch_seed(EPOCH))],
            len: 0,
        };
        let b0 = log::encode_group(&g0, log::epoch_seed(EPOCH));
        ext[..b0.len()].copy_from_slice(&b0);
        let g1 = log::Group {
            lsn: 138,
            records: vec![rec(12, 3, 138, Payload::Noop(160), None)],
            len: 0,
        };
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let b1 = log::encode_group(&g1, seed);
        ext[138..138 + b1.len()].copy_from_slice(&b1);
        let slot = initial_slot(1);
        let lc = check_log(&[(1, &ext)], &slot).unwrap();
        assert_eq!(
            (lc.scan.groups.len(), lc.scan.end, lc.zero_tail),
            (2, 338, true)
        );
        let mut stale = ext.clone();
        stale[400] = 1;
        assert!(!check_log(&[(1, &stale)], &slot).unwrap().zero_tail);
        let mut bad = ext;
        bad[200] ^= 1;
        let lc = check_log(&[(1, &bad)], &slot).unwrap();
        assert!(matches!(lc.scan.stop, log::Stop::Corrupt(_)) && lc.scan.end == 138);
    }

    /// [F04 §6], [F05 §10.2]: a slot that is the fold of the log it covers passes; a counter or pointer it does not
    /// derive fails. [F05 §5.3]: after the end of a scan, whole records decode detached and a damaged one is stepped over.
    #[test]
    fn head_fold_and_tail() {
        use crate::head::tests::initial_slot;
        use crate::log::tests::{EPOCH, head, rec};
        let mut ext = vec![0u8; 1 << 16];
        let g0 = log::Group {
            lsn: 0,
            records: vec![head(0, log::epoch_seed(EPOCH))],
            len: 0,
        };
        let b0 = log::encode_group(&g0, log::epoch_seed(EPOCH));
        ext[..b0.len()].copy_from_slice(&b0);
        let g1 = log::Group {
            lsn: 138,
            records: vec![rec(12, 3, 138, Payload::Noop(160), None)],
            len: 0,
        };
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let b1 = log::encode_group(&g1, seed);
        ext[138..138 + b1.len()].copy_from_slice(&b1);
        let mut slot = initial_slot(1);
        (slot.next_ref_id, slot.hlc_seq, slot.refs_lsn) = (0, 0, 0);
        let lc = check_log(&[(1, &ext)], &slot).unwrap();
        check_head_fold(&slot, &lc.scan).unwrap();
        let mut wrong = slot.clone();
        wrong.next_id = 2;
        let e = check_head_fold(&wrong, &lc.scan).unwrap_err();
        assert!(e.reason.contains("next_id"), "{e}");
        let mut wrong = slot.clone();
        wrong.refs_lsn = 138;
        assert!(check_head_fold(&wrong, &lc.scan).is_err());
        // A stale Noop group left after the end of the valid log (338), then a torn one.
        let stale = log::encode_group(
            &log::Group {
                lsn: 338,
                records: vec![rec(12, 3, 338, Payload::Noop(8), None)],
                len: 0,
            },
            7,
        );
        let mut torn = stale.clone();
        torn[35] ^= 1;
        ext[338..338 + stale.len()].copy_from_slice(&stale);
        ext[338 + stale.len()..338 + 2 * stale.len()].copy_from_slice(&torn);
        let recs = walk_tail(&ext, 338).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].decoded, Ok(12));
        assert!(recs[1].decoded.is_err());
        assert!(walk_tail(&ext, 338 + 2 * stale.len()).unwrap().is_empty());
    }

    /// [F05 §7] rows 29 and 30, §10.2: a `BodyDrop` and a `Harvest` raise `hlc_seq` to their `hlc`. A log of an extent
    /// head and one record of each kind folds to a slot with that `hlc_seq`; a slot that kept 0 is refused.
    #[test]
    fn head_fold_body_drop_and_harvest() {
        use crate::head::tests::initial_slot;
        use crate::log::tests::{EPOCH, head, rec};
        const X1: u64 = (2 << 16) | 1;
        const X2: u64 = (3 << 16) | 2;
        let body_drop = Payload::BodyDrop(Box::new(log::BodyDrop {
            hashes: vec![[5; 16]],
            reason: 1,
            note: String::new(),
            actor: 0,
            hlc: X1,
            origin: 2,
        }));
        let harvest = Payload::Harvest(Box::new(log::Harvest {
            source: "claude:s".into(),
            op: log::HarvestOp::Mark {
                path: "t".into(),
                head: [0xAB; 16],
                seen: 3,
            },
            hlc: X2,
        }));
        let mut fails = Vec::new();
        for (what, kind, payload, hlc) in [
            ("BodyDrop", 29, body_drop, X1),
            ("Harvest", 30, harvest, X2),
        ] {
            let mut ext = vec![0u8; 1 << 16];
            let b0 = log::encode_group(
                &log::Group {
                    lsn: 0,
                    records: vec![head(0, log::epoch_seed(EPOCH))],
                    len: 0,
                },
                log::epoch_seed(EPOCH),
            );
            ext[..b0.len()].copy_from_slice(&b0);
            let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
            let b1 = log::encode_group(
                &log::Group {
                    lsn: 138,
                    records: vec![rec(kind, 2, 138, payload, None)],
                    len: 0,
                },
                seed,
            );
            ext[138..138 + b1.len()].copy_from_slice(&b1);
            let end = 138 + b1.len() as u64;
            let mut slot = initial_slot(1);
            (slot.next_ref_id, slot.refs_lsn) = (0, 0);
            (slot.committed_lsn, slot.durable_lsn, slot.hlc_seq) = (end, end, hlc);
            let lc = match check_log(&[(1, &ext)], &slot) {
                Ok(lc) => lc,
                Err(e) => {
                    fails.push(format!("{what}: the log: {e}"));
                    continue;
                }
            };
            if lc.scan.end != end {
                fails.push(format!("{what}: the scan ends at {}", lc.scan.end));
            }
            if let Err(e) = check_head_fold(&slot, &lc.scan) {
                fails.push(format!("{what}: hlc_seq {hlc}: {e}"));
            }
            slot.hlc_seq = 0;
            match check_head_fold(&slot, &lc.scan) {
                Err(e) if e.reason.contains("hlc_seq") => {}
                other => fails.push(format!("{what}: hlc_seq 0: {other:?}")),
            }
        }
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    /// An [F11 §2.1] section of `tag` from its header fields, its rows and its heap, decoded by
    /// `runtime::decode_section` in a segment of kind `seg`.
    fn rt_section(
        tag: u16,
        form: u8,
        (n_rows, row_size): (usize, u16),
        rows: &[u8],
        heap: &[u8],
        seg: runtime::SegKind,
    ) -> runtime::Section {
        let mut w = Writer::new();
        w.u32(n_rows as u32);
        w.u16(row_size);
        w.u8(form);
        w.u8(0);
        w.u32(0);
        w.u32(0);
        w.u64(heap.len() as u64);
        w.bytes(rows);
        w.bytes(heap);
        let t = runtime::Table::from_tag(tag).expect("tag");
        runtime::decode_section(t, &w.into_vec(), 0, seg).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A `DROPPED` section ([F11 §13.4]) of rows (hash, `drop_lsn`, `hlc`, `reason`), sorted by hash; `None` (absent)
    /// with no row.
    fn dropped_sec(
        rows: &[([u8; 16], u64, u64, u8)],
        seg: runtime::SegKind,
    ) -> Option<runtime::Section> {
        let mut v = rows.to_vec();
        v.sort();
        let mut w = Writer::new();
        for (h, lsn, hlc, reason) in &v {
            w.bytes(h);
            w.u64(*lsn);
            w.u64(*hlc);
            w.u8(*reason);
            w.zeros(7);
        }
        (!v.is_empty()).then(|| rt_section(0x020D, 1, (v.len(), 40), &w.into_vec(), &[], seg))
    }

    /// A `HARVEST` section ([F11 §13.5]) of (source, live row or `None` for a dead row), sorted by source: each row's
    /// 56 fixed bytes with its `HeapRef`s at the running heap offset, then its `source`, `path` and `ranges` slices
    /// (§2.3); the snapshot form in a base, the layer form in a delta; `None` (absent) with no row.
    fn harvest_sec(
        rows: &[(&str, Option<&HarvestRow>)],
        seg: runtime::SegKind,
    ) -> Option<runtime::Section> {
        let mut v = rows.to_vec();
        v.sort_by_key(|r| r.0);
        let mut fixed = Writer::new();
        let mut heap = Vec::new();
        for (source, row) in &v {
            let dead = HarvestRow::default();
            let r = row.unwrap_or(&dead);
            fixed.bytes(&r.head);
            fixed.u64(r.seen);
            fixed.u8(if row.is_some() { 0 } else { 0x80 });
            fixed.zeros(7);
            let mut ranges = Writer::new();
            for x in &r.ranges {
                ranges.u64(x.start);
                ranges.u64(x.end);
                ranges.bytes(&x.commit);
                ranges.u64(x.commit_lsn);
            }
            for s in [source.as_bytes(), r.path.as_bytes(), &ranges.into_vec()] {
                fixed.u32(heap.len() as u32);
                fixed.u32(s.len() as u32);
                heap.extend_from_slice(s);
            }
        }
        let form = if seg == runtime::SegKind::Base { 1 } else { 2 };
        (!v.is_empty())
            .then(|| rt_section(0x020E, form, (v.len(), 56), &fixed.into_vec(), &heap, seg))
    }

    fn body_drop(hashes: &[[u8; 16]], reason: u8, hlc: u64) -> Payload {
        Payload::BodyDrop(Box::new(log::BodyDrop {
            hashes: hashes.to_vec(),
            reason,
            note: String::new(),
            actor: 0,
            hlc,
            origin: 1,
        }))
    }

    fn harvest(source: &str, op: log::HarvestOp) -> Payload {
        Payload::Harvest(Box::new(log::Harvest {
            source: source.into(),
            op,
            hlc: 0,
        }))
    }

    fn mark(path: &str, head: u8, seen: u64) -> log::HarvestOp {
        log::HarvestOp::Mark {
            path: path.into(),
            head: [head; 16],
            seen,
        }
    }

    /// A `Commit` record whose `commit_id` is 32 × `id`, so its `id16` is 16 × `id`.
    fn commit_rec(id: u8) -> Payload {
        Payload::Commit(Box::new(commit::Commit {
            commit_id: [id; 32],
            ..commit::tests::base_commit()
        }))
    }

    fn hrow(head: u8, seen: u64, path: &str, ranges: &[(u64, u64, u8, u64)]) -> HarvestRow {
        HarvestRow {
            head: [head; 16],
            seen,
            path: path.into(),
            ranges: ranges
                .iter()
                .map(|&(start, end, c, commit_lsn)| runtime::HarvestRange {
                    start,
                    end,
                    commit: [c; 16],
                    commit_lsn,
                })
                .collect(),
        }
    }

    fn recs(start: u64, end: u64, v: &[(u64, Payload)]) -> FoldRecords<'_> {
        FoldRecords {
            start,
            end,
            records: v.iter().map(|(l, p)| (*l, p)).collect(),
        }
    }

    fn pass(fails: &mut Vec<String>, what: &str, r: Result<()>) {
        if let Err(e) = r {
            fails.push(format!("{what}: refused: {e}"));
        }
    }

    fn refused(fails: &mut Vec<String>, what: &str, r: Result<()>, cites: &[&str]) {
        match r {
            Ok(()) => fails.push(format!("{what}: accepted")),
            Err(e) if cites.iter().any(|c| !e.reason.contains(c)) => {
                fails.push(format!("{what}: want {cites:?}, got {e}"));
            }
            Err(_) => {}
        }
    }

    /// [F05 §10.3] into [F11 §13.4]: a segment's `DROPPED` section is the rows below plus a row per hash of each
    /// `BodyDrop` of its window `[from, upto)`, the first row staying, with its own `drop_lsn`, `hlc` and `reason`;
    /// conditional (§2.4); the records must cover the window.
    #[test]
    fn dropped_fold() {
        use runtime::SegKind::{Base, Delta};
        const A: u64 = (1 << 16) | 1;
        const B: u64 = (2 << 16) | 1;
        const C: u64 = (3 << 16) | 1;
        let (h1, h2, h3, h9) = ([1u8; 16], [2u8; 16], [3u8; 16], [9u8; 16]);
        let none = TableView::default();
        let mut fails = Vec::new();
        let base = |rows: &[([u8; 16], u64, u64, u8)], r: &FoldRecords<'_>, below: &TableView| {
            check_table_folds(
                Base,
                (0, 1000),
                dropped_sec(rows, Base).as_ref(),
                None,
                below,
                r,
            )
        };
        let delta = |rows: &[([u8; 16], u64, u64, u8)], r: &FoldRecords<'_>, below: &TableView| {
            check_table_folds(
                Delta,
                (500, 1000),
                dropped_sec(rows, Delta).as_ref(),
                None,
                below,
                r,
            )
        };
        // D-1: a hash dropped twice keeps its first row.
        let v = [
            (100, body_drop(&[h1, h2], 1, A)),
            (300, body_drop(&[h1], 2, B)),
        ];
        let r = recs(0, 1000, &v);
        pass(
            &mut fails,
            "D-1",
            base(&[(h1, 100, A, 1), (h2, 100, A, 1)], &r, &none),
        );
        refused(
            &mut fails,
            "D-1 the later row",
            base(&[(h1, 300, B, 2), (h2, 100, A, 1)], &r, &none),
            &["§13.4", "drop_lsn"],
        );
        // D-2: drop_lsn is the lsn of the record that dropped the hash.
        let v = [(100, body_drop(&[h1], 1, A)), (300, body_drop(&[h2], 1, B))];
        let r = recs(0, 1000, &v);
        pass(
            &mut fails,
            "D-2",
            base(&[(h1, 100, A, 1), (h2, 300, B, 1)], &r, &none),
        );
        refused(
            &mut fails,
            "D-2 another BodyDrop's lsn",
            base(&[(h1, 300, A, 1), (h2, 300, B, 1)], &r, &none),
            &["§13.4", "drop_lsn"],
        );
        // hlc and reason are the record's own, each compared on its own.
        for (what, rows, field) in [
            (
                "D-2 another BodyDrop's hlc",
                [(h1, 100, B, 1), (h2, 300, B, 1)],
                "hlc",
            ),
            (
                "D-2 another reason",
                [(h1, 100, A, 2), (h2, 300, B, 1)],
                "reason",
            ),
        ] {
            refused(&mut fails, what, base(&rows, &r, &none), &["§13.4", field]);
        }
        // D-3: a window above 0 over the previous segment's rows.
        let mut below = TableView::default();
        below.dropped.insert(
            h1,
            DroppedRow {
                drop_lsn: 100,
                hlc: A,
                reason: 1,
            },
        );
        let v = [
            (200, body_drop(&[h9], 1, A)),
            (600, body_drop(&[h1, h3], 3, C)),
        ];
        let r = recs(0, 1000, &v);
        pass(
            &mut fails,
            "D-3",
            delta(&[(h1, 100, A, 1), (h3, 600, C, 3)], &r, &below),
        );
        for (what, rows, cite) in [
            (
                "D-3 a row from below the window",
                vec![(h1, 100, A, 1), (h3, 600, C, 3), (h9, 200, A, 1)],
                "not given by the fold",
            ),
            (
                "D-3 a row below replaced",
                vec![(h1, 600, C, 3), (h3, 600, C, 3)],
                "drop_lsn",
            ),
            ("D-3 a row below lost", vec![(h3, 600, C, 3)], "no row"),
        ] {
            refused(&mut fails, what, delta(&rows, &r, &below), &["§13.4", cite]);
        }
        // D-4: the section is carried exactly when it has a row.
        let r = recs(0, 1000, &[]);
        pass(&mut fails, "D-4", base(&[], &r, &none));
        refused(
            &mut fails,
            "D-4 a row with no BodyDrop",
            base(&[(h1, 100, A, 1)], &r, &none),
            &["§13.4", "not given by the fold"],
        );
        let v = [(400, body_drop(&[h1], 1, A))];
        refused(
            &mut fails,
            "D-4 a BodyDrop with no section",
            base(&[], &recs(0, 1000, &v), &none),
            &["§13.4", "no row"],
        );
        // D-5: the records must cover the window.
        pass(&mut fails, "D-5", delta(&[], &recs(500, 1000, &[]), &none));
        for (what, s, e) in [("D-5 from 600", 600, 1000), ("D-5 to 900", 500, 900)] {
            refused(
                &mut fails,
                what,
                delta(&[], &recs(s, e, &[]), &none),
                &["F05 §10.3", "window"],
            );
        }
        // D-7: the window is [from, upto): a BodyDrop at from_lsn is folded, one at upto_lsn is not ([F09 §2.3]).
        let v = [
            (500, body_drop(&[h1], 1, A)),
            (1000, body_drop(&[h2], 2, B)),
        ];
        let r = recs(500, 1100, &v);
        pass(&mut fails, "D-7", delta(&[(h1, 500, A, 1)], &r, &none));
        refused(
            &mut fails,
            "D-7 a row for the BodyDrop at upto_lsn",
            delta(&[(h1, 500, A, 1), (h2, 1000, B, 2)], &r, &none),
            &["§13.4", "not given by the fold"],
        );
        refused(
            &mut fails,
            "D-7 no row for the BodyDrop at from_lsn",
            delta(&[], &r, &none),
            &["§13.4", "no row"],
        );
        // D-6: the segment wrapper reads the kind, the window and the section by tag.
        let seg = segment::tests::small_base();
        pass(
            &mut fails,
            "D-6",
            check_segment_folds(&seg, &none, &recs(0, 1000, &[])),
        );
        let v = [(500, body_drop(&[h1], 1, A))];
        let r = recs(0, 1000, &v);
        let mut with = seg.clone();
        let at = with
            .sections
            .iter()
            .position(|(e, _)| e.tag > 0x020D)
            .unwrap_or(with.sections.len());
        with.sections.insert(
            at,
            (
                segment::tests::ent(0x020D, 1),
                segment::Sec::Runtime(Box::new(
                    dropped_sec(&[(h1, 500, A, 1)], Base).expect("a row"),
                )),
            ),
        );
        pass(
            &mut fails,
            "D-6 with a section",
            check_segment_folds(&with, &none, &r),
        );
        refused(
            &mut fails,
            "D-6 a BodyDrop with no section",
            check_segment_folds(&seg, &none, &r),
            &["§13.4", "no row"],
        );
        let mut cs = seg.clone();
        cs.hdr.seg_kind = 9;
        refused(
            &mut fails,
            "D-6 seg_kind 9",
            check_segment_folds(&cs, &none, &r),
            &["§2.4"],
        );
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    /// [F05 §10.3] into [F11 §13.5]: a `HARVEST` base is the snapshot of the fold, a delta's layer holds every source its
    /// window touched with its full state, or dead when it has no row at `upto_lsn` (R-70); a range's `commit_lsn` is the
    /// lsn of the `Commit` it names; a forget above 0 keeps the ranges below it; the last mark sets `head`, `seen` and
    /// `path`; only the records of `[from, upto)` fold; the view under a layer composes base and delta layers (§2.4).
    #[test]
    fn harvest_fold() {
        use runtime::SegKind::{Base, Delta};
        let mut fails = Vec::new();
        // H-1: a range's commit_lsn is the lsn of the Commit record it names.
        let v = [
            (100, harvest("claude:a", mark("p1", 1, 10))),
            (150, commit_rec(0xC1)),
            (
                160,
                harvest(
                    "claude:a",
                    log::HarvestOp::Range {
                        start: 0,
                        end: 10,
                        commit: [0xC1; 16],
                    },
                ),
            ),
            (200, harvest("claude:a", mark("p2", 2, 20))),
        ];
        let r = recs(0, 500, &v);
        let none = TableView::default();
        let base = |rows: &[(&str, Option<&HarvestRow>)]| {
            check_table_folds(
                Base,
                (0, 500),
                None,
                harvest_sec(rows, Base).as_ref(),
                &none,
                &r,
            )
        };
        let a = hrow(2, 20, "p2", &[(0, 10, 0xC1, 150)]);
        pass(&mut fails, "H-1", base(&[("claude:a", Some(&a))]));
        let a160 = hrow(2, 20, "p2", &[(0, 10, 0xC1, 160)]);
        refused(
            &mut fails,
            "H-1 the Harvest record's lsn",
            base(&[("claude:a", Some(&a160))]),
            &["§13.5", "commit_lsn"],
        );
        // The last mark sets head, seen and path, each compared on its own: one field is the first mark's.
        for (what, row, field) in [
            (
                "H-1 the first mark's head",
                hrow(1, 20, "p2", &[(0, 10, 0xC1, 150)]),
                "head",
            ),
            (
                "H-1 the first mark's seen",
                hrow(2, 10, "p2", &[(0, 10, 0xC1, 150)]),
                "seen",
            ),
            (
                "H-1 the first mark's path",
                hrow(2, 20, "p1", &[(0, 10, 0xC1, 150)]),
                "path",
            ),
        ] {
            refused(
                &mut fails,
                what,
                base(&[("claude:a", Some(&row))]),
                &["§13.5", field],
            );
        }
        // H-8: the view under a layer: a base, then a delta layer with a dead and a changed row.
        let b = hrow(3, 90, "pb", &[(0, 50, 0xC1, 150), (50, 80, 0xC1, 150)]);
        let c = hrow(4, 30, "pc", &[]);
        let u = hrow(5, 40, "pu", &[]);
        let y = hrow(6, 1, "py", &[]);
        let z = hrow(7, 1, "pz", &[]);
        let z2 = hrow(8, 2, "pz2", &[]);
        let mut view = TableView::default();
        let base_d = dropped_sec(&[([1; 16], 100, 1, 1)], Base);
        let base_h = harvest_sec(
            &[
                ("claude:a", Some(&a)),
                ("claude:b", Some(&b)),
                ("claude:c", Some(&c)),
                ("claude:u", Some(&u)),
                ("claude:y", Some(&y)),
                ("claude:z", Some(&z)),
            ],
            Base,
        );
        let layer = harvest_sec(&[("claude:y", None), ("claude:z", Some(&z2))], Delta);
        pass(
            &mut fails,
            "H-8 base",
            view.push(Base, base_d.as_ref(), base_h.as_ref()),
        );
        pass(
            &mut fails,
            "H-8 delta",
            view.push(Delta, None, layer.as_ref()),
        );
        let want: BTreeMap<String, HarvestRow> = [
            ("claude:a", &a),
            ("claude:b", &b),
            ("claude:c", &c),
            ("claude:u", &u),
            ("claude:z", &z2),
        ]
        .into_iter()
        .map(|(s, r)| (s.to_owned(), r.clone()))
        .collect();
        if view.harvest != want || !view.dropped.is_empty() {
            fails.push(format!("H-8: the view is {view:?}"));
        }
        // H-2 to H-6: one delta [500, 1000) over that view, its records covering [0, 1000).
        let range = |start: u64, end: u64, c: u8| log::HarvestOp::Range {
            start,
            end,
            commit: [c; 16],
        };
        let v = [
            (300, harvest("codex:d", mark("pd", 9, 5))),
            (650, commit_rec(0xC2)),
            (660, harvest("claude:a", range(10, 20, 0xC2))),
            (
                700,
                harvest("claude:b", log::HarvestOp::Forget { from: 50 }),
            ),
            (710, harvest("claude:c", log::HarvestOp::Forget { from: 0 })),
            (720, harvest("codex:e", mark("pe", 0xE, 7))),
            (730, harvest("codex:e", log::HarvestOp::Forget { from: 0 })),
            (740, harvest("claude:u", mark("pu", 5, 40))),
        ];
        let r = recs(0, 1000, &v);
        let delta = |rows: &[(&str, Option<&HarvestRow>)]| {
            check_table_folds(
                Delta,
                (500, 1000),
                None,
                harvest_sec(rows, Delta).as_ref(),
                &view,
                &r,
            )
        };
        let a2 = hrow(2, 20, "p2", &[(0, 10, 0xC1, 150), (10, 20, 0xC2, 650)]);
        let b2 = hrow(3, 90, "pb", &[(0, 50, 0xC1, 150)]);
        let clean: Vec<(&str, Option<&HarvestRow>)> = vec![
            ("claude:a", Some(&a2)),
            ("claude:b", Some(&b2)),
            ("claude:c", None),
            ("claude:u", Some(&u)),
            ("codex:e", None),
        ];
        pass(&mut fails, "H-2..H-6", delta(&clean));
        let a_only = hrow(2, 20, "p2", &[(10, 20, 0xC2, 650)]);
        let d = hrow(9, 5, "pd", &[]);
        /// The clean layer with the row of `key` left out, or replaced by `row` when given.
        fn with<'a>(
            clean: &[(&'a str, Option<&'a HarvestRow>)],
            key: &'a str,
            row: Option<Option<&'a HarvestRow>>,
        ) -> Vec<(&'a str, Option<&'a HarvestRow>)> {
            let mut v: Vec<_> = clean.iter().filter(|x| x.0 != key).copied().collect();
            if let Some(row) = row {
                v.push((key, row));
            }
            v
        }
        let variant = |key, row| with(&clean, key, row);
        for (what, rows, cites) in [
            (
                "H-2 a forget above 0 kept both ranges",
                variant("claude:b", Some(Some(&b))),
                &["§13.5", "ranges"][..],
            ),
            (
                "H-3 a forget at 0 left no dead row",
                variant("claude:c", None),
                &["§13.5", "no row", "dead"][..],
            ),
            (
                "H-4 a layer row without the ranges below",
                variant("claude:a", Some(Some(&a_only))),
                &["§13.5", "ranges"][..],
            ),
            (
                "H-5 a mark below the window",
                variant("codex:d", Some(Some(&d))),
                &["§13.5", "not given by the fold"][..],
            ),
            (
                "H-6 a source created and forgotten left no dead row",
                variant("codex:e", None),
                &["§13.5", "no row", "dead"][..],
            ),
            (
                "H-6 an untouched source's row",
                variant("claude:z", Some(Some(&z2))),
                &["§13.5", "not given by the fold"][..],
            ),
            (
                "H-6 a touched source with its value below left no row",
                variant("claude:u", None),
                &["§13.5", "no row"][..],
            ),
        ] {
            refused(&mut fails, what, delta(&rows), cites);
        }
        // H-7: a range for a source with no row, and one naming a commit no earlier Commit record carries.
        for (what, op_src, cites) in [
            (
                "H-7 a range with no row",
                "codex:f",
                &["§13.5", "no row"][..],
            ),
            (
                "H-7 a range naming no commit",
                "claude:a",
                &["§13.5", "no earlier Commit"][..],
            ),
        ] {
            let c = if op_src == "codex:f" { 0xC2 } else { 0xC3 };
            let v = [
                (650, commit_rec(0xC2)),
                (660, harvest(op_src, range(0, 5, c))),
            ];
            let r = recs(0, 1000, &v);
            refused(
                &mut fails,
                what,
                check_table_folds(Delta, (500, 1000), None, None, &view, &r),
                cites,
            );
        }
        // H-9: the window is [from, upto): a Harvest at from_lsn is folded, one at upto_lsn is not ([F09 §2.3]).
        let v = [
            (500, harvest("codex:g", mark("pg", 0xA, 1))),
            (1000, harvest("codex:h", mark("ph", 0xB, 2))),
        ];
        let r = recs(500, 1100, &v);
        let g = hrow(0xA, 1, "pg", &[]);
        let h = hrow(0xB, 2, "ph", &[]);
        let edge = |rows: &[(&str, Option<&HarvestRow>)]| {
            check_table_folds(
                Delta,
                (500, 1000),
                None,
                harvest_sec(rows, Delta).as_ref(),
                &none,
                &r,
            )
        };
        pass(&mut fails, "H-9", edge(&[("codex:g", Some(&g))]));
        refused(
            &mut fails,
            "H-9 a row for the Harvest at upto_lsn",
            edge(&[("codex:g", Some(&g)), ("codex:h", Some(&h))]),
            &["§13.5", "not given by the fold"],
        );
        refused(
            &mut fails,
            "H-9 no row for the Harvest at from_lsn",
            edge(&[]),
            &["§13.5", "no row"],
        );
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    /// [F02 §6.3] names and [F09 §17.1] V-3: a segment whose `file_no` is not its name's is refused.
    #[test]
    fn file_names_and_v3() {
        let p = |s: &str| FileName::parse(s).map(|n| (n.family, n.ref_id, n.file_no));
        assert_eq!(p("seg.base.10"), Some((3, 0, 10)));
        assert_eq!(p("seg.d7"), Some((4, 0, 7)));
        assert_eq!(p("seg.b0.1"), Some((5, 0, 1)));
        assert_eq!(p("seg.b1.5"), Some((5, 1, 5)));
        assert_eq!(p("cs.6"), Some((9, 0, 6)));
        assert_eq!(p("gitmap.3"), Some((8, 0, 3)));
        for bad in [
            "log.1",
            "seg.d07",
            "seg.b01.1",
            "cs.0",
            "blobs.4294967296",
            "HEAD",
            "seg.dx",
        ] {
            assert_eq!(p(bad), None, "{bad}");
        }
        for n in ["seg.b1.5", "hist.12", "dict.2"] {
            assert_eq!(FileName::parse(n).unwrap().name(), n);
        }
        let seg = segment::encode_segment(&segment::tests::small_base());
        assert_eq!(
            sealed_identity("seg.base.5", &seg).unwrap().total_len,
            seg.len() as u64
        );
        assert!(sealed_identity("seg.base.6", &seg).is_err());
        assert!(sealed_identity("seg.d5", &seg).is_err());
        assert!(check_file(&seg, Some("seg.base.6")).is_err());
        assert_eq!(check_file(&seg, Some("seg.base.5")).unwrap(), Kind::Segment);
    }

    /// Fragments: a run of op values and one of canonical values round-trip; trailing garbage fails.
    #[test]
    fn fragments() {
        let mut w = Writer::new();
        Value::Int(-3).encode(&mut w);
        Value::Bool(true).encode(&mut w);
        assert_eq!(
            check_fragment(&Fragment::OpValues, w.as_slice()).unwrap(),
            2
        );
        let cv = [canon::Cv::Absent.encode(), canon::Cv::Int(5).encode()].concat();
        assert_eq!(check_fragment(&Fragment::CanonicalValues, &cv).unwrap(), 2);
        let mut bad = cv.clone();
        bad.push(8);
        assert!(check_fragment(&Fragment::CanonicalValues, &bad).is_err());
    }

    /// [F08 §8.5] "Stored key form" and "Item key order", [F06 §7.6]: with a fragment's symbols of class `name`, schema
    /// items must be strictly ascending by (class, stored key) and a `Schema` op's `item_key` must be its item's stored
    /// key form; an item naming a symbol the fragment does not define is refused rather than left unchecked.
    #[test]
    fn fragment_schema_names() {
        use crate::value::ItemBody;
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let kind = Item {
            iflags: 0,
            body: ItemBody::Kind {
                name: 1,
                kind_id: 64,
                uid_derivation: 1,
                root_variant: 0,
                existence_policy: 2,
                kflags: 4,
            },
        };
        let value = |name: u32| Item {
            iflags: 0,
            body: ItemBody::EnumValue {
                kind: 1,
                field: 2,
                name,
                value: 0,
                sort_rank: 0,
                eflags: 0,
                covers: vec![],
            },
        };
        let policy = Item {
            iflags: 0,
            body: ItemBody::Policy {
                name: "policy.role.developer.mcp-write".into(),
                value: "yes".into(),
            },
        };
        let items = |v: &[&Item]| {
            let mut w = Writer::new();
            for it in v {
                it.encode(&mut w);
            }
            w.as_slice().to_vec()
        };
        let all = names(&["incident", "status", "open", "wontfix"]);
        let good = items(&[&kind, &value(3), &value(4), &policy]);
        let f = Fragment::SchemaItems { names: all.clone() };
        assert_eq!(check_fragment(&f, &good).unwrap(), 4);
        let swapped = items(&[&kind, &value(4), &value(3), &policy]);
        let e = check_fragment(&f, &swapped).unwrap_err();
        assert!(e.reason.contains("item key order"), "{e}");
        let twice = items(&[&kind, &value(3), &value(3)]);
        assert!(check_fragment(&f, &twice).is_err());
        let short = Fragment::SchemaItems {
            names: names(&["incident", "status"]),
        };
        let e = check_fragment(&short, &good).unwrap_err();
        assert!(e.reason.contains("does not define"), "{e}");

        let schema = |key: &[u8], it: &Item| {
            let op = commit::Op::Schema {
                mode: 0,
                item_class: it.class(),
                item_key: key.to_vec(),
                old: None,
                new: Some(it.clone()),
            };
            let mut w = Writer::new();
            op.encode(&mut w);
            w.as_slice().to_vec()
        };
        let frames = |n: Vec<String>| Fragment::OpFrames {
            lsn: 30_000,
            names: n,
        };
        let ok = [
            schema(b"incident", &kind),
            schema(b"incident\0status\0open", &value(3)),
        ]
        .concat();
        assert_eq!(check_fragment(&frames(all.clone()), &ok).unwrap(), 2);
        let wrong = schema(b"incident\0status\0wontfix", &value(3));
        let e = check_fragment(&frames(all), &wrong).unwrap_err();
        assert!(e.reason.contains("stored key form"), "{e}");
        let e = check_fragment(&frames(names(&["incident"])), &ok).unwrap_err();
        assert!(e.reason.contains("does not define"), "{e}");
    }
}
