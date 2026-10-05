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
//! - What a sealed file states about itself for the references that name it ([F09 §17.1] V-8) is [`Sealed`].
//! - A fragment, a sequence of one structure to the end of the file, by [`check_fragment`].
//! - The `.expect` and `case.txt` files by [`parse_framed`] and [`hex_block`] (the framing of
//!   `fixtures/canonical/INDEX.md` §2.1 and §2.3).

use crate::head::{HeadFile, HeadSlot, SlotClass};
use crate::lock::LockFile;
use crate::log::{self, Payload, Scan};
use crate::prim::{Error, Reader, Result, Writer, err};
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
