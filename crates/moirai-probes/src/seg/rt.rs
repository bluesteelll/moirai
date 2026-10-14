//! The section body of [F11 §2.1] (`11-runtime-tables.md:88-108`) and the rows a delta checkpoint's store-level and
//! runtime sections hold: `RtHdr`, the rows, the index region and the heap; the row sizes and forms of [F11 §1.3]
//! (`:49-76`) and of `SCHEMAIDS` and `FILES` ([F09 §14.3]–§14.4, `09-segments.md:968-1009`); the rows of `REFS` with its
//! name index ([F11 §3], `:300-446`), `IDEM` as a hash table ([F11 §8], `:633-667`), `ALLOC` and `UIDX` ([F11 §9],
//! `:669-714`), `TREES` and `FILEOBS` ([F11 §12.4]–§12.5, `:826-960`) and `FILES`; the bodies of the tables with no row;
//! and a reader for the tests.
//!
//! - **The body** (§2.1): `RtHdr` (24 B: `n_rows`, `row_size`, `form` 1 snapshot or 2 layer, `_reserved`, `aux`,
//!   `index_len`, `heap_len`), then `n_rows` rows of `row_size` bytes, the index region, the heap, with no gap; the
//!   section's `len` is `24 + n_rows × row_size + index_len + heap_len`. A section with no row is still written.
//! - **The heap** (§2.3, `:117-140`): every slice in row order and, within a row, in the order of its `HeapRef` fields,
//!   with no gap; each `HeapRef` is (`off`, `len`) with `off` the running offset, also when `len` is 0.
//! - **`IDEM`** (§8): an open-addressing table of capacity C, the smallest power of two ≥ max(16, 2n); an entry's home
//!   slot is `u64le(key[0..8]) & (C − 1)`; entries are inserted in ascending key order by linear probing; an empty slot
//!   is 72 zero bytes; `aux` = n. An entry whose origin is a `Commit` has no inline result: `result` is (0, 0).
//! - **`ALLOC`** (§9.1): a dense array from `aux`; a layer with no row has `aux` 0. A hole (`create_seq` 0) is refused:
//!   the probe's windows allocate contiguous ids.

use crate::overlay::{AllocRow, UidxRow};
use crate::seg::tag;
use crate::tail::rt::{EPOCH_LEN, Epoch, FILEOBS_ROW_LEN, HEAP_REF_LEN};

/// `RtHdr`'s length (`11-runtime-tables.md:93-102`).
pub const RT_HDR_LEN: usize = 24;

/// `RtHdr.form` (`11-runtime-tables.md:97`, §2.4).
pub mod form {
    /// `snapshot`: the complete table as of the segment's `upto_lsn`.
    pub const SNAPSHOT: u8 = 1;
    /// `layer`: the rows that changed after the next-older segment.
    pub const LAYER: u8 = 2;
}

/// The row size of `tag`'s section ([F11 §1.3], `11-runtime-tables.md:49-76`; `SCHEMAIDS` and `FILES`,
/// `09-segments.md:975-985`, `:998-1009`); `None` for a tag whose body is not [F11 §2.1]'s.
pub fn row_size(t: u16) -> Option<u16> {
    Some(match t {
        tag::REFS => 164,
        tag::PINS => 21,
        tag::HEADS => 161,
        tag::LEASES => 180,
        tag::MARKERS | tag::MARKERS_OLD | tag::IDEM => 72,
        tag::ALLOC => 24,
        tag::UIDX | tag::GLOBIDX => 20,
        tag::CONFLICTS
        | tag::CURSORS
        | tag::SESSMARKS
        | tag::BACKUPS
        | tag::HARVEST
        | tag::FILES => 56,
        tag::DROPPED => 40,
        tag::TREES => 201,
        tag::FILEOBS => 187,
        tag::PENDING => 123,
        tag::FSINTENT => 132,
        tag::FPRINT => 54,
        tag::JOURNALCUR => 41,
        tag::DIRMAP => 91,
        tag::PREFIXEV => 52,
        tag::ANCESTRY => 67,
        tag::GITRENAMES => 98,
        tag::ANCHORRES => 78,
        tag::SCHEMAIDS => 16,
        _ => return None,
    })
}

/// The form of `tag`'s section in a delta segment ([F11 §1.3] "Form (base / delta)"; versioned `CONFLICTS` and
/// `GLOBIDX` are layers in a delta, §2.4; `SCHEMAIDS` a layer, `FILES` a snapshot, [F09 §14.3]–§14.4). Every one is a
/// snapshot in a base.
pub fn delta_form(t: u16) -> Option<u8> {
    use form::{LAYER, SNAPSHOT};
    Some(match t {
        tag::REFS
        | tag::PINS
        | tag::HEADS
        | tag::LEASES
        | tag::MARKERS
        | tag::BACKUPS
        | tag::DROPPED
        | tag::TREES
        | tag::FSINTENT
        | tag::JOURNALCUR
        | tag::PREFIXEV
        | tag::FILES => SNAPSHOT,
        tag::MARKERS_OLD
        | tag::IDEM
        | tag::ALLOC
        | tag::UIDX
        | tag::CURSORS
        | tag::SESSMARKS
        | tag::HARVEST
        | tag::FILEOBS
        | tag::PENDING
        | tag::FPRINT
        | tag::DIRMAP
        | tag::ANCESTRY
        | tag::GITRENAMES
        | tag::ANCHORRES
        | tag::CONFLICTS
        | tag::GLOBIDX
        | tag::SCHEMAIDS => LAYER,
        _ => return None,
    })
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().expect("2 bytes"))
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

fn b16(b: &[u8], at: usize) -> [u8; 16] {
    b[at..at + 16].try_into().expect("16 bytes")
}

/// `RtHdr` (`11-runtime-tables.md:93-102`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RtHdr {
    /// `n_rows` (offset 0): the rows, or `IDEM`'s slots.
    pub n_rows: u32,
    /// `row_size` (4, `u16`).
    pub row_size: u16,
    /// `form` (6).
    pub form: u8,
    /// `aux` (8).
    pub aux: u32,
    /// `index_len` (12).
    pub index_len: u32,
    /// `heap_len` (16, `u64`).
    pub heap_len: u64,
}

impl RtHdr {
    /// The 24 bytes; byte 7 is `_reserved`, zero.
    pub fn encode(&self) -> [u8; RT_HDR_LEN] {
        let mut b = [0u8; RT_HDR_LEN];
        b[0..4].copy_from_slice(&self.n_rows.to_le_bytes());
        b[4..6].copy_from_slice(&self.row_size.to_le_bytes());
        b[6] = self.form;
        b[8..12].copy_from_slice(&self.aux.to_le_bytes());
        b[12..16].copy_from_slice(&self.index_len.to_le_bytes());
        b[16..24].copy_from_slice(&self.heap_len.to_le_bytes());
        b
    }

    /// Decodes 24 bytes; refuses a form other than 1 or 2 and a non-zero `_reserved`.
    pub fn decode(b: &[u8]) -> Result<RtHdr, String> {
        if b.len() < RT_HDR_LEN {
            return Err(format!("an RtHdr of {} bytes", b.len()));
        }
        if !matches!(b[6], form::SNAPSHOT | form::LAYER) || b[7] != 0 {
            return Err(format!("RtHdr form {} or reserved {}", b[6], b[7]));
        }
        Ok(RtHdr {
            n_rows: u32_le(b, 0),
            row_size: u16_le(b, 4),
            form: b[6],
            aux: u32_le(b, 8),
            index_len: u32_le(b, 12),
            heap_len: u64_le(b, 16),
        })
    }
}

/// Appends a body: `RtHdr`, `rows` (whole rows of `row_size`), `index`, `heap`.
pub fn put_body(
    out: &mut Vec<u8>,
    row_size: u16,
    form: u8,
    aux: u32,
    rows: &[u8],
    index: &[u8],
    heap: &[u8],
) -> Result<(), String> {
    if row_size == 0 || !rows.len().is_multiple_of(usize::from(row_size)) {
        return Err(format!("{} row bytes of rows of {row_size}", rows.len()));
    }
    let h = RtHdr {
        n_rows: u32::try_from(rows.len() / usize::from(row_size)).map_err(|_| "rows")?,
        row_size,
        form,
        aux,
        index_len: u32::try_from(index.len()).map_err(|_| "index")?,
        heap_len: heap.len() as u64,
    };
    out.reserve(RT_HDR_LEN + rows.len() + index.len() + heap.len());
    out.extend_from_slice(&h.encode());
    out.extend_from_slice(rows);
    out.extend_from_slice(index);
    out.extend_from_slice(heap);
    Ok(())
}

/// Appends the body of a table with no row: `tag`'s row size, `form`, `aux` 0. `IDEM` is never row-less (§2.1); its
/// empty body is [`put_idem`]'s.
pub fn put_empty(out: &mut Vec<u8>, t: u16, form: u8) -> Result<(), String> {
    if t == tag::IDEM {
        return put_idem(out, &[]);
    }
    let rs = row_size(t).ok_or_else(|| format!("{t:#06x} has no F11 body"))?;
    put_body(out, rs, form, 0, &[], &[], &[])
}

/// A parsed body.
#[derive(Copy, Clone, Debug)]
pub struct Body<'a> {
    /// The header.
    pub hdr: RtHdr,
    /// The rows.
    pub rows: &'a [u8],
    /// The index region.
    pub index: &'a [u8],
    /// The heap.
    pub heap: &'a [u8],
}

impl<'a> Body<'a> {
    /// Parses a section's bytes: `row_size` must be `want`, and the length identity hold (§2.1, §2.7).
    pub fn parse(b: &'a [u8], want: u16) -> Result<Body<'a>, String> {
        let hdr = RtHdr::decode(b)?;
        if hdr.row_size != want {
            return Err(format!("row_size {}, not {want}", hdr.row_size));
        }
        let rows = hdr.n_rows as usize * usize::from(want);
        let total = (RT_HDR_LEN + rows + hdr.index_len as usize) as u64 + hdr.heap_len;
        if total != b.len() as u64 {
            return Err(format!(
                "a body of {} bytes whose RtHdr gives {total}",
                b.len()
            ));
        }
        let index_at = RT_HDR_LEN + rows;
        let heap_at = index_at + hdr.index_len as usize;
        Ok(Body {
            hdr,
            rows: &b[RT_HDR_LEN..index_at],
            index: &b[index_at..heap_at],
            heap: &b[heap_at..],
        })
    }

    /// Row `i`.
    pub fn row(&self, i: usize) -> &'a [u8] {
        let w = usize::from(self.hdr.row_size);
        &self.rows[w * i..w * (i + 1)]
    }

    /// The heap slice of the `HeapRef` at byte `at` of `row`; refuses one out of bounds (§2.3).
    pub fn slice(&self, row: &[u8], at: usize) -> Result<&'a [u8], String> {
        let (off, len) = (u32_le(row, at) as usize, u32_le(row, at + 4) as usize);
        self.heap.get(off..off + len).ok_or_else(|| {
            format!(
                "a HeapRef ({off}, {len}) past the heap's {}",
                self.heap.len()
            )
        })
    }
}

/// A heap under construction: each slice at the running offset (§2.3).
#[derive(Clone, Debug, Default)]
pub struct Heap(pub Vec<u8>);

impl Heap {
    /// Appends `s` and returns its `HeapRef`'s 8 bytes.
    pub fn push(&mut self, s: &[u8]) -> [u8; HEAP_REF_LEN] {
        let mut r = [0u8; HEAP_REF_LEN];
        r[0..4].copy_from_slice(&(self.0.len() as u32).to_le_bytes());
        r[4..8].copy_from_slice(&(s.len() as u32).to_le_bytes());
        self.0.extend_from_slice(s);
        r
    }
}

/// One `REFS` row ([F11 §3.1], `11-runtime-tables.md:306-333`), its heap slices decoded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefRow {
    /// `ref_id` (offset 0).
    pub ref_id: u32,
    /// `kind` (4): 1 `work` …
    pub kind: u8,
    /// `flags` (5).
    pub flags: u8,
    /// `tip` (8, `b16`).
    pub tip: [u8; 16],
    /// `tip_lsn` (24).
    pub tip_lsn: u64,
    /// `gen` (32).
    pub gen_no: u32,
    /// `ref_seq_next` (36).
    pub ref_seq_next: u32,
    /// `base_pin` (40).
    pub base_pin: u64,
    /// `fork_commit` (48, `b16`).
    pub fork_commit: [u8; 16],
    /// `fork_lsn` (64).
    pub fork_lsn: u64,
    /// `fork_seq` (72).
    pub fork_seq: u64,
    /// `fork_ref_id` (80).
    pub fork_ref_id: u32,
    /// `ops_since_fork` (84).
    pub ops_since_fork: u32,
    /// `overlay_ops` (88).
    pub overlay_ops: u32,
    /// `overlay_bytes` (92).
    pub overlay_bytes: u32,
    /// `promoted_seg` (96).
    pub promoted_seg: u32,
    /// `ops_total` (100).
    pub ops_total: u64,
    /// `bytes_total` (108).
    pub bytes_total: u64,
    /// `trunk_mark_ops` (116).
    pub trunk_mark_ops: u64,
    /// `trunk_mark_bytes` (124).
    pub trunk_mark_bytes: u64,
    /// `name` (132, `HeapRef` `text`).
    pub name: String,
    /// `absorbed` (140, `list<AbsorbedEntry>`): (`ref_id`, `ref_seq`) sorted by `ref_id`, unique (§3.4).
    pub absorbed: Vec<(u32, u32)>,
    /// `moves` (148, `list<u64>`): at most 32, newest first (§3.5).
    pub moves: Vec<u64>,
    /// `message` (156, `text`).
    pub message: String,
}

/// `REFS`' move cache length (`11-runtime-tables.md:371`).
pub const MOVES_MAX: usize = 32;
/// `REFS`' row size.
pub const REFS_ROW: usize = 164;

/// Appends `REFS` in `form` with `aux` = `next_ref_id`: the rows, sorted by `ref_id`, then the name index (the row
/// positions ordered by (`name`, `ref_id`), §3.3), then the heap.
pub fn put_refs(
    out: &mut Vec<u8>,
    form: u8,
    rows: &[RefRow],
    next_ref_id: u32,
) -> Result<(), String> {
    let mut fixed = Vec::with_capacity(REFS_ROW * rows.len());
    let mut heap = Heap::default();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 && rows[i - 1].ref_id >= r.ref_id {
            return Err(format!(
                "REFS rows {} then {}: not by ref_id",
                rows[i - 1].ref_id,
                r.ref_id
            ));
        }
        if r.moves.len() > MOVES_MAX || r.absorbed.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(format!(
                "ref {}: {} moves or an unsorted absorbed vector",
                r.ref_id,
                r.moves.len()
            ));
        }
        let at = fixed.len();
        fixed.resize(at + REFS_ROW, 0);
        let b = &mut fixed[at..];
        b[0..4].copy_from_slice(&r.ref_id.to_le_bytes());
        b[4] = r.kind;
        b[5] = r.flags;
        b[8..24].copy_from_slice(&r.tip);
        b[24..32].copy_from_slice(&r.tip_lsn.to_le_bytes());
        b[32..36].copy_from_slice(&r.gen_no.to_le_bytes());
        b[36..40].copy_from_slice(&r.ref_seq_next.to_le_bytes());
        b[40..48].copy_from_slice(&r.base_pin.to_le_bytes());
        b[48..64].copy_from_slice(&r.fork_commit);
        b[64..72].copy_from_slice(&r.fork_lsn.to_le_bytes());
        b[72..80].copy_from_slice(&r.fork_seq.to_le_bytes());
        b[80..84].copy_from_slice(&r.fork_ref_id.to_le_bytes());
        b[84..88].copy_from_slice(&r.ops_since_fork.to_le_bytes());
        b[88..92].copy_from_slice(&r.overlay_ops.to_le_bytes());
        b[92..96].copy_from_slice(&r.overlay_bytes.to_le_bytes());
        b[96..100].copy_from_slice(&r.promoted_seg.to_le_bytes());
        b[100..108].copy_from_slice(&r.ops_total.to_le_bytes());
        b[108..116].copy_from_slice(&r.bytes_total.to_le_bytes());
        b[116..124].copy_from_slice(&r.trunk_mark_ops.to_le_bytes());
        b[124..132].copy_from_slice(&r.trunk_mark_bytes.to_le_bytes());
        let absorbed: Vec<u8> = r
            .absorbed
            .iter()
            .flat_map(|(id, seq)| id.to_le_bytes().into_iter().chain(seq.to_le_bytes()))
            .collect();
        let moves: Vec<u8> = r.moves.iter().flat_map(|m| m.to_le_bytes()).collect();
        b[132..140].copy_from_slice(&heap.push(r.name.as_bytes()));
        b[140..148].copy_from_slice(&heap.push(&absorbed));
        b[148..156].copy_from_slice(&heap.push(&moves));
        b[156..164].copy_from_slice(&heap.push(r.message.as_bytes()));
    }
    let mut order: Vec<u32> = (0..rows.len() as u32).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&rows[a as usize], &rows[b as usize]);
        (x.name.as_bytes(), x.ref_id).cmp(&(y.name.as_bytes(), y.ref_id))
    });
    let index: Vec<u8> = order.iter().flat_map(|p| p.to_le_bytes()).collect();
    put_body(
        out,
        REFS_ROW as u16,
        form,
        next_ref_id,
        &fixed,
        &index,
        &heap.0,
    )
}

/// Reads `REFS` back: its rows and `aux`; refuses a name index that is not the permutation of §3.3.
pub fn decode_refs(b: &[u8]) -> Result<(Vec<RefRow>, u32), String> {
    let body = Body::parse(b, REFS_ROW as u16)?;
    let n = body.hdr.n_rows as usize;
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        let r = body.row(i);
        let text = |at: usize| -> Result<String, String> {
            String::from_utf8(body.slice(r, at)?.to_vec()).map_err(|e| e.to_string())
        };
        let absorbed = body.slice(r, 140)?;
        let moves = body.slice(r, 148)?;
        if !absorbed.len().is_multiple_of(8) || !moves.len().is_multiple_of(8) {
            return Err(format!("row {i}: a list slice not of whole entries"));
        }
        rows.push(RefRow {
            ref_id: u32_le(r, 0),
            kind: r[4],
            flags: r[5],
            tip: b16(r, 8),
            tip_lsn: u64_le(r, 24),
            gen_no: u32_le(r, 32),
            ref_seq_next: u32_le(r, 36),
            base_pin: u64_le(r, 40),
            fork_commit: b16(r, 48),
            fork_lsn: u64_le(r, 64),
            fork_seq: u64_le(r, 72),
            fork_ref_id: u32_le(r, 80),
            ops_since_fork: u32_le(r, 84),
            overlay_ops: u32_le(r, 88),
            overlay_bytes: u32_le(r, 92),
            promoted_seg: u32_le(r, 96),
            ops_total: u64_le(r, 100),
            bytes_total: u64_le(r, 108),
            trunk_mark_ops: u64_le(r, 116),
            trunk_mark_bytes: u64_le(r, 124),
            name: text(132)?,
            absorbed: absorbed
                .chunks(8)
                .map(|c| (u32_le(c, 0), u32_le(c, 4)))
                .collect(),
            moves: moves.chunks(8).map(|c| u64_le(c, 0)).collect(),
            message: text(156)?,
        });
    }
    let mut again = Vec::new();
    put_refs(&mut again, body.hdr.form, &rows, body.hdr.aux)?;
    if again != b {
        return Err("REFS is not in the canonical form of its rows (index, heap order)".into());
    }
    Ok((rows, body.hdr.aux))
}

/// One `IDEM` entry ([F11 §8], `11-runtime-tables.md:638-650`) of a `Commit`'s `idem_key`/`idem_payload`: no inline
/// result, so `result` is (0, 0).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct IdemEntry {
    /// `key` (offset 0).
    pub key: [u8; 16],
    /// `payload` (16).
    pub payload: [u8; 16],
    /// `ref_id` (32).
    pub ref_id: u32,
    /// `branch_sym` (36).
    pub branch_sym: u32,
    /// `ref_seq` (40).
    pub ref_seq: u32,
    /// `flags` (44): bit 0 `used`, bit 1 `default_key`.
    pub flags: u8,
    /// `append_hlc` (48).
    pub append_hlc: u64,
    /// `origin_lsn` (56).
    pub origin_lsn: u64,
}

/// `IDEM`'s row size.
pub const IDEM_ROW: usize = 72;

/// `IDEM`'s capacity for `n` entries: the smallest power of two ≥ max(16, 2n) (§8).
pub fn idem_capacity(n: usize) -> usize {
    (2 * n).max(16).next_power_of_two()
}

fn idem_home(key: &[u8; 16], cap: usize) -> usize {
    (u64::from_le_bytes(key[0..8].try_into().expect("8 bytes")) & (cap as u64 - 1)) as usize
}

/// Appends `IDEM` (a layer): the entries inserted in ascending key order by linear probing from their home slots;
/// `aux` = the number of entries. Refuses two entries with one key, and an entry without `used`.
pub fn put_idem(out: &mut Vec<u8>, entries: &[IdemEntry]) -> Result<(), String> {
    let mut sorted: Vec<&IdemEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.key);
    if sorted.windows(2).any(|w| w[0].key == w[1].key) || sorted.iter().any(|e| e.flags & 1 == 0) {
        return Err("IDEM entries with one key, or without used".into());
    }
    let cap = idem_capacity(sorted.len());
    let mut slots = vec![0u8; cap * IDEM_ROW];
    for e in sorted {
        let mut s = idem_home(&e.key, cap);
        while slots[s * IDEM_ROW + 44] & 1 != 0 {
            s = (s + 1) % cap;
        }
        let b = &mut slots[s * IDEM_ROW..(s + 1) * IDEM_ROW];
        b[0..16].copy_from_slice(&e.key);
        b[16..32].copy_from_slice(&e.payload);
        b[32..36].copy_from_slice(&e.ref_id.to_le_bytes());
        b[36..40].copy_from_slice(&e.branch_sym.to_le_bytes());
        b[40..44].copy_from_slice(&e.ref_seq.to_le_bytes());
        b[44] = e.flags;
        b[48..56].copy_from_slice(&e.append_hlc.to_le_bytes());
        b[56..64].copy_from_slice(&e.origin_lsn.to_le_bytes());
    }
    put_body(
        out,
        IDEM_ROW as u16,
        form::LAYER,
        entries.len() as u32,
        &slots,
        &[],
        &[],
    )
}

fn idem_at(b: &[u8]) -> IdemEntry {
    IdemEntry {
        key: b16(b, 0),
        payload: b16(b, 16),
        ref_id: u32_le(b, 32),
        branch_sym: u32_le(b, 36),
        ref_seq: u32_le(b, 40),
        flags: b[44],
        append_hlc: u64_le(b, 48),
        origin_lsn: u64_le(b, 56),
    }
}

/// Looks `key` up in an `IDEM` body (§8 "Lookup"): from its home slot until an equal key or an unused slot.
pub fn idem_lookup(body: &Body<'_>, key: &[u8; 16]) -> Option<IdemEntry> {
    let cap = body.hdr.n_rows as usize;
    let mut s = idem_home(key, cap);
    for _ in 0..cap {
        let r = body.row(s);
        if r[44] & 1 == 0 {
            return None;
        }
        if r[0..16] == key[..] {
            return Some(idem_at(r));
        }
        s = (s + 1) % cap;
    }
    None
}

/// Reads `IDEM` back: (slot, entry) of every used slot; refuses a non-zero empty slot, a capacity that is not
/// [`idem_capacity`] of `aux`, or `aux` other than the used slots.
pub fn decode_idem(b: &[u8]) -> Result<Vec<(usize, IdemEntry)>, String> {
    let body = Body::parse(b, IDEM_ROW as u16)?;
    let mut out = Vec::new();
    for s in 0..body.hdr.n_rows as usize {
        let r = body.row(s);
        if r[44] & 1 == 0 {
            if r.iter().any(|&x| x != 0) {
                return Err(format!("IDEM slot {s} is unused but not zero"));
            }
            continue;
        }
        out.push((s, idem_at(r)));
    }
    let n = body.hdr.aux as usize;
    if out.len() != n || body.hdr.n_rows as usize != idem_capacity(n) {
        return Err(format!(
            "IDEM: {} used slots, aux {n}, capacity {}",
            out.len(),
            body.hdr.n_rows
        ));
    }
    Ok(out)
}

/// Appends `ALLOC` in `form`: rows `#first`, `#first + 1`, …, `aux` = `first` (0 with no row, §9.1). A hole
/// (`create_seq` 0) is refused.
pub fn put_alloc(out: &mut Vec<u8>, form: u8, first: u32, rows: &[AllocRow]) -> Result<(), String> {
    if rows.is_empty() != (first == 0) {
        return Err(format!("ALLOC from #{first} with {} rows", rows.len()));
    }
    let mut b = Vec::with_capacity(24 * rows.len());
    for (i, r) in rows.iter().enumerate() {
        if r.create_seq == 0 {
            return Err(format!("ALLOC #{}: a hole", first as usize + i));
        }
        b.extend_from_slice(&r.uid);
        b.extend_from_slice(&r.ref_id.to_le_bytes());
        b.extend_from_slice(&r.create_seq.to_le_bytes());
    }
    put_body(out, 24, form, first, &b, &[], &[])
}

/// Reads `ALLOC` back: (`aux`, rows).
pub fn decode_alloc(b: &[u8]) -> Result<(u32, Vec<AllocRow>), String> {
    let body = Body::parse(b, 24)?;
    let rows = (0..body.hdr.n_rows as usize)
        .map(|i| {
            let r = body.row(i);
            AllocRow {
                uid: b16(r, 0),
                ref_id: u32_le(r, 16),
                create_seq: u32_le(r, 20),
            }
        })
        .collect();
    Ok((body.hdr.aux, rows))
}

/// Appends `UIDX` in `form`: (`uid`, `n`) rows, strictly ascending by `uid` (§9.2).
pub fn put_uidx(out: &mut Vec<u8>, form: u8, rows: &[UidxRow]) -> Result<(), String> {
    if rows.windows(2).any(|w| w[0].uid >= w[1].uid) {
        return Err("UIDX rows are not strictly ascending by uid".into());
    }
    let mut b = Vec::with_capacity(20 * rows.len());
    for r in rows {
        b.extend_from_slice(&r.uid);
        b.extend_from_slice(&r.n.to_le_bytes());
    }
    put_body(out, 20, form, 0, &b, &[], &[])
}

/// Reads `UIDX` back.
pub fn decode_uidx(b: &[u8]) -> Result<Vec<UidxRow>, String> {
    let body = Body::parse(b, 20)?;
    Ok((0..body.hdr.n_rows as usize)
        .map(|i| {
            let r = body.row(i);
            UidxRow {
                uid: b16(r, 0),
                n: u32_le(r, 16),
            }
        })
        .collect())
}

/// Appends a settle epoch's 32 bytes ([F11 §12.4], `11-runtime-tables.md:855-864`).
pub fn put_epoch(out: &mut Vec<u8>, e: &Epoch) {
    out.push(e.scope_kind);
    out.extend_from_slice(&[0; 3]);
    out.extend_from_slice(&e.scope_ref.to_le_bytes());
    out.extend_from_slice(&e.digest);
    out.extend_from_slice(&e.hlc.to_le_bytes());
}

/// One `TREES` row ([F11 §12.4], `11-runtime-tables.md:830-845`), its heap slices decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeRow {
    /// `key` (offset 0).
    pub key: [u8; 16],
    /// `os` (16).
    pub os: u8,
    /// `flags` (17).
    pub flags: u8,
    /// `root_id` (18, `OsFileId`).
    pub root_id: [u8; 57],
    /// `caps` (75, `VolumeCaps`).
    pub caps: [u8; 16],
    /// `last_head` (91, `OidSlot`).
    pub last_head: [u8; 33],
    /// `last_settle_hlc` (124).
    pub last_settle_hlc: u64,
    /// `dirty_count` (132).
    pub dirty_count: u32,
    /// `dirty_head` (136, `OidSlot`).
    pub dirty_head: [u8; 33],
    /// `dirty_hlc` (169).
    pub dirty_hlc: u64,
    /// `root_text` (177, `text`).
    pub root_text: String,
    /// `sens` (185): the encoded `list<SensEntry>`.
    pub sens: Vec<u8>,
    /// `epochs` (193, `list<Epoch>`): at most one per (`scope_kind`, `scope_ref`), sorted by it.
    pub epochs: Vec<Epoch>,
}

/// `TREES`' row size.
pub const TREES_ROW: usize = 201;

/// Appends `TREES` (a snapshot): rows strictly ascending by `key`, then the heap.
pub fn put_trees(out: &mut Vec<u8>, rows: &[TreeRow]) -> Result<(), String> {
    let mut fixed = Vec::with_capacity(TREES_ROW * rows.len());
    let mut heap = Heap::default();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 && rows[i - 1].key >= r.key {
            return Err("TREES rows are not strictly ascending by key".into());
        }
        if r.epochs
            .windows(2)
            .any(|w| (w[0].scope_kind, w[0].scope_ref) >= (w[1].scope_kind, w[1].scope_ref))
        {
            return Err("a tree's epochs are not one per scope, sorted".into());
        }
        let at = fixed.len();
        fixed.resize(at + TREES_ROW, 0);
        let b = &mut fixed[at..];
        b[0..16].copy_from_slice(&r.key);
        b[16] = r.os;
        b[17] = r.flags;
        b[18..75].copy_from_slice(&r.root_id);
        b[75..91].copy_from_slice(&r.caps);
        b[91..124].copy_from_slice(&r.last_head);
        b[124..132].copy_from_slice(&r.last_settle_hlc.to_le_bytes());
        b[132..136].copy_from_slice(&r.dirty_count.to_le_bytes());
        b[136..169].copy_from_slice(&r.dirty_head);
        b[169..177].copy_from_slice(&r.dirty_hlc.to_le_bytes());
        let mut epochs = Vec::with_capacity(EPOCH_LEN * r.epochs.len());
        for e in &r.epochs {
            put_epoch(&mut epochs, e);
        }
        b[177..185].copy_from_slice(&heap.push(r.root_text.as_bytes()));
        b[185..193].copy_from_slice(&heap.push(&r.sens));
        b[193..201].copy_from_slice(&heap.push(&epochs));
    }
    put_body(
        out,
        TREES_ROW as u16,
        form::SNAPSHOT,
        0,
        &fixed,
        &[],
        &heap.0,
    )
}

/// Reads `TREES` back.
pub fn decode_trees(b: &[u8]) -> Result<Vec<TreeRow>, String> {
    let body = Body::parse(b, TREES_ROW as u16)?;
    let mut rows = Vec::new();
    for i in 0..body.hdr.n_rows as usize {
        let r = body.row(i);
        let ep = body.slice(r, 193)?;
        if !ep.len().is_multiple_of(EPOCH_LEN) {
            return Err(format!("tree {i}: epochs of {} bytes", ep.len()));
        }
        rows.push(TreeRow {
            key: b16(r, 0),
            os: r[16],
            flags: r[17],
            root_id: r[18..75].try_into().expect("57 bytes"),
            caps: b16(r, 75),
            last_head: r[91..124].try_into().expect("33 bytes"),
            last_settle_hlc: u64_le(r, 124),
            dirty_count: u32_le(r, 132),
            dirty_head: r[136..169].try_into().expect("33 bytes"),
            dirty_hlc: u64_le(r, 169),
            root_text: String::from_utf8(body.slice(r, 177)?.to_vec())
                .map_err(|e| e.to_string())?,
            sens: body.slice(r, 185)?.to_vec(),
            epochs: ep
                .chunks(EPOCH_LEN)
                .map(|c| Epoch {
                    scope_kind: c[0],
                    scope_ref: u32_le(c, 4),
                    digest: b16(c, 8),
                    hlc: u64_le(c, 24),
                })
                .collect(),
        });
    }
    Ok(rows)
}

/// Appends `FILEOBS` (a layer) from row images ([F11 §2.9]): each the 187 fixed bytes with `var` = (0, slice length),
/// then the slice. The rows go in the order given, which must be strictly ascending by (`n`, `tree`); each `var` gets
/// its running heap offset.
pub fn put_fileobs(out: &mut Vec<u8>, images: &[&[u8]]) -> Result<(), String> {
    let mut fixed = Vec::with_capacity(FILEOBS_ROW_LEN * images.len());
    let mut heap = Heap::default();
    let key = |im: &[u8]| (u32_le(im, 0), b16(im, 4));
    for (i, im) in images.iter().enumerate() {
        if im.len() < FILEOBS_ROW_LEN
            || u32_le(im, 179) != 0
            || u32_le(im, 183) as usize != im.len() - FILEOBS_ROW_LEN
        {
            return Err(format!(
                "FILEOBS image {i} of {} bytes is not an F11 §2.9 image",
                im.len()
            ));
        }
        if i > 0 && key(images[i - 1]) >= key(im) {
            return Err("FILEOBS rows are not strictly ascending by (n, tree)".into());
        }
        let at = fixed.len();
        fixed.extend_from_slice(&im[..FILEOBS_ROW_LEN]);
        let var = heap.push(&im[FILEOBS_ROW_LEN..]);
        fixed[at + 179..at + 187].copy_from_slice(&var);
    }
    put_body(
        out,
        FILEOBS_ROW_LEN as u16,
        form::LAYER,
        0,
        &fixed,
        &[],
        &heap.0,
    )
}

/// Reads `FILEOBS` back as row images (`var` = (0, slice length), then the slice).
pub fn decode_fileobs(b: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let body = Body::parse(b, FILEOBS_ROW_LEN as u16)?;
    let mut out = Vec::new();
    for i in 0..body.hdr.n_rows as usize {
        let r = body.row(i);
        let s = body.slice(r, 179)?;
        let mut im = r.to_vec();
        im[179..183].copy_from_slice(&[0; 4]);
        im.extend_from_slice(s);
        out.push(im);
    }
    Ok(out)
}

/// One `FILES` row ([F09 §14.4], `09-segments.md:998-1009`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FileRow {
    /// `file.family` (offset 0).
    pub family: u8,
    /// `file.ref_id` (1).
    pub ref_id: u32,
    /// `file.file_no` (5).
    pub file_no: u32,
    /// `dest` (9).
    pub dest: u8,
    /// `algo` (10).
    pub algo: u8,
    /// `flags` (11).
    pub flags: u8,
    /// `from_lsn` (16).
    pub from_lsn: u64,
    /// `upto_lsn` (24).
    pub upto_lsn: u64,
    /// `total_len` (32).
    pub total_len: u64,
    /// `digest16` (40).
    pub digest16: [u8; 16],
}

impl FileRow {
    /// The key `file` (`family`, `ref_id`, `file_no`).
    pub fn key(&self) -> (u8, u32, u32) {
        (self.family, self.ref_id, self.file_no)
    }
}

/// Appends `FILES` (a snapshot): rows strictly ascending by `file` ([F11 §2.5] `FileRef`).
pub fn put_files(out: &mut Vec<u8>, rows: &[FileRow]) -> Result<(), String> {
    if rows.windows(2).any(|w| w[0].key() >= w[1].key()) {
        return Err("FILES rows are not strictly ascending by file".into());
    }
    let mut b = Vec::with_capacity(56 * rows.len());
    for r in rows {
        b.push(r.family);
        b.extend_from_slice(&r.ref_id.to_le_bytes());
        b.extend_from_slice(&r.file_no.to_le_bytes());
        b.extend_from_slice(&[r.dest, r.algo, r.flags, 0, 0, 0, 0]);
        b.extend_from_slice(&r.from_lsn.to_le_bytes());
        b.extend_from_slice(&r.upto_lsn.to_le_bytes());
        b.extend_from_slice(&r.total_len.to_le_bytes());
        b.extend_from_slice(&r.digest16);
    }
    put_body(out, 56, form::SNAPSHOT, 0, &b, &[], &[])
}

/// Reads `FILES` back.
pub fn decode_files(b: &[u8]) -> Result<Vec<FileRow>, String> {
    let body = Body::parse(b, 56)?;
    Ok((0..body.hdr.n_rows as usize)
        .map(|i| {
            let r = body.row(i);
            FileRow {
                family: r[0],
                ref_id: u32_le(r, 1),
                file_no: u32_le(r, 5),
                dest: r[9],
                algo: r[10],
                flags: r[11],
                from_lsn: u64_le(r, 16),
                upto_lsn: u64_le(r, 24),
                total_len: u64_le(r, 32),
                digest16: b16(r, 40),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::rt::FileObsRow;

    /// The tags whose body is [F11 §2.1]'s, with the row size and the delta form F11 §1.3 (and F09 §14.3–§14.4)
    /// give them, written out from the chapters' tables.
    const TABLE: [(u16, u16, u8); 30] = [
        (0x0031, 56, 2),
        (0x0084, 20, 2),
        (0x0101, 16, 2),
        (0x0102, 56, 1),
        (0x0201, 164, 1),
        (0x0202, 21, 1),
        (0x0203, 161, 1),
        (0x0204, 180, 1),
        (0x0205, 72, 1),
        (0x0206, 72, 2),
        (0x0207, 72, 2),
        (0x0208, 24, 2),
        (0x0209, 20, 2),
        (0x020A, 56, 2),
        (0x020B, 56, 2),
        (0x020C, 56, 1),
        (0x020D, 40, 1),
        (0x020E, 56, 2),
        (0x0210, 201, 1),
        (0x0211, 187, 2),
        (0x0212, 123, 2),
        (0x0213, 132, 1),
        (0x0214, 54, 2),
        (0x0215, 41, 1),
        (0x0216, 91, 2),
        (0x0217, 52, 1),
        (0x0218, 67, 2),
        (0x0219, 98, 2),
        (0x021A, 78, 2),
        (0x0040, 0, 0),
    ];

    /// The section body ([F11 §2.1], `11-runtime-tables.md:88-108`): `RtHdr` is 24 bytes with `n_rows` at 0, `row_size`
    /// at 4, `form` at 6, `_reserved` at 7, `aux` at 8, `index_len` at 12, `heap_len` at 16; the body's length is the
    /// identity; an empty `IDEM` holds 16 zero slots with `aux` 0; every other table with no row is a bare `RtHdr`; a
    /// wrong `row_size`, a cut body and a form other than 1 or 2 are refused; and each tag's row size and delta form are
    /// the chapters' (`:49-76`; `09-segments.md:975-985`, `:998-1009`).
    #[test]
    fn rt_bodies_follow_f11_2_1() {
        let h = RtHdr {
            n_rows: 0x0403_0201,
            row_size: 0x0605,
            form: 2,
            aux: 0x0C0B_0A09,
            index_len: 0x100F_0E0D,
            heap_len: 0x1817_1615_1413_1211,
        };
        let mut want: Vec<u8> = (1..=6).collect();
        want.extend([2, 0]);
        want.extend(9..=0x18);
        assert_eq!(h.encode().to_vec(), want);
        assert_eq!(RtHdr::decode(&want), Ok(h));
        let mut idem = Vec::new();
        put_empty(&mut idem, tag::IDEM, form::LAYER).unwrap();
        assert_eq!(
            idem.len(),
            24 + 16 * 72,
            "an empty IDEM: 16 zero slots wanted, {} bytes written",
            idem.len()
        );
        assert_eq!(&idem[0..12], &[16, 0, 0, 0, 72, 0, 2, 0, 0, 0, 0, 0]);
        assert!(idem[24..].iter().all(|&b| b == 0));
        assert_eq!(decode_idem(&idem), Ok(Vec::new()));
        let mut pins = Vec::new();
        put_empty(&mut pins, tag::PINS, form::SNAPSHOT).unwrap();
        assert_eq!(
            pins,
            RtHdr {
                row_size: 21,
                form: 1,
                ..RtHdr::default()
            }
            .encode()
        );
        // The length identity: two UIDX rows, no index or heap.
        let mut u = Vec::new();
        put_body(&mut u, 20, form::LAYER, 0, &[7u8; 40], &[], &[]).unwrap();
        assert_eq!(u.len(), 24 + 40);
        let body = Body::parse(&u, 20).unwrap();
        assert_eq!((body.hdr.n_rows, body.row(1)), (2, &[7u8; 20][..]));
        assert!(Body::parse(&u, 24).unwrap_err().contains("row_size"));
        assert!(Body::parse(&u[..u.len() - 1], 20).is_err(), "a cut body");
        let mut bad = u.clone();
        bad[6] = 3;
        assert!(Body::parse(&bad, 20).is_err(), "form 3");
        assert!(put_body(&mut Vec::new(), 20, 2, 0, &[0; 30], &[], &[]).is_err());
        // The chapters' table.
        for (t, rs, f) in TABLE {
            let (want_rs, want_f) = if rs == 0 {
                (None, None)
            } else {
                (Some(rs), Some(f))
            };
            assert_eq!(
                (row_size(t), delta_form(t)),
                (want_rs, want_f),
                "tag {t:#06x}"
            );
        }
    }

    fn sample_refs() -> Vec<RefRow> {
        vec![
            RefRow {
                ref_id: 0,
                kind: 1,
                name: "main".into(),
                tip: [0xA1; 16],
                tip_lsn: 0x1122_3344_5566_7788,
                gen_no: 0x0102_0304,
                ref_seq_next: 0x0506_0708,
                absorbed: vec![(0, 7)],
                moves: vec![900, 800],
                ops_total: 0x0909_0909_0909_0909,
                ..RefRow::default()
            },
            RefRow {
                ref_id: 2,
                kind: 1,
                flags: 0,
                name: "lane/a".into(),
                fork_commit: [0xA1; 16],
                fork_lsn: 777,
                fork_seq: 6,
                fork_ref_id: 0,
                absorbed: vec![(0, 6), (2, 3)],
                ..RefRow::default()
            },
            RefRow {
                ref_id: 5,
                kind: 5,
                flags: 2,
                name: "lane/B".into(),
                message: "v1".into(),
                trunk_mark_bytes: 0x5A,
                ..RefRow::default()
            },
        ]
    }

    /// `REFS` ([F11 §3.1], `11-runtime-tables.md:306-333`): 164-byte rows sorted by `ref_id`, each field at its offset;
    /// the heap holds each row's `name`, `absorbed`, `moves` and `message` in row order, each `HeapRef` at the running
    /// offset; the index region is the row positions ordered by (`name`, `ref_id`) (§3.3); `aux` is `next_ref_id`. It
    /// reads back, and rows out of `ref_id` order are refused.
    #[test]
    fn refs_snapshot_rows_and_name_index_follow_f11_3() {
        let rows = sample_refs();
        let mut b = Vec::new();
        put_refs(&mut b, form::SNAPSHOT, &rows, 6).unwrap();
        let body = Body::parse(&b, 164).unwrap();
        assert_eq!(
            (
                body.hdr.n_rows,
                body.hdr.form,
                body.hdr.aux,
                body.hdr.index_len
            ),
            (3, 1, 6, 12)
        );
        let r0 = body.row(0);
        assert_eq!(&r0[0..4], &[0, 0, 0, 0]);
        assert_eq!((r0[4], r0[5], &r0[6..8]), (1, 0, &[0u8, 0][..]));
        assert_eq!(&r0[8..24], &[0xA1; 16], "tip at 8");
        assert_eq!(
            &r0[24..32],
            &0x1122_3344_5566_7788u64.to_le_bytes(),
            "tip_lsn at 24"
        );
        assert_eq!(&r0[32..36], &[4, 3, 2, 1], "gen at 32");
        assert_eq!(&r0[36..40], &[8, 7, 6, 5], "ref_seq_next at 36");
        assert_eq!(&r0[100..108], &[9; 8], "ops_total at 100");
        let r1 = body.row(1);
        assert_eq!(&r1[48..64], &[0xA1; 16], "fork_commit at 48");
        assert_eq!(&r1[64..72], &777u64.to_le_bytes(), "fork_lsn at 64");
        assert_eq!(&r1[72..80], &6u64.to_le_bytes(), "fork_seq at 72");
        let r2 = body.row(2);
        assert_eq!((r2[4], r2[5]), (5, 2), "kind tag, flags pinned");
        assert_eq!(
            &r2[124..132],
            &0x5Au64.to_le_bytes(),
            "trunk_mark_bytes at 124"
        );
        // The heap: "main", absorbed [(0, 7)], moves [900, 800], "", then "lane/a", [(0, 6), (2, 3)], [], "", then
        // "lane/B", [], [], "v1".
        let mut heap = b"main".to_vec();
        heap.extend([0, 0, 0, 0, 7, 0, 0, 0]);
        heap.extend(900u64.to_le_bytes());
        heap.extend(800u64.to_le_bytes());
        heap.extend(b"lane/a");
        heap.extend([0, 0, 0, 0, 6, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
        heap.extend(b"lane/B");
        heap.extend(b"v1");
        assert_eq!(body.heap, &heap[..]);
        let href = |r: &[u8], at: usize| (u32_le(r, at), u32_le(r, at + 4));
        assert_eq!(
            [href(r0, 132), href(r0, 140), href(r0, 148), href(r0, 156)],
            [(0, 4), (4, 8), (12, 16), (28, 0)]
        );
        assert_eq!(
            [href(r1, 132), href(r1, 140), href(r1, 148), href(r1, 156)],
            [(28, 6), (34, 16), (50, 0), (50, 0)]
        );
        assert_eq!(href(r2, 156), (56, 2));
        // "lane/B" < "lane/a" < "main" bytewise: positions 2, 1, 0.
        assert_eq!(body.index, &[2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(decode_refs(&b), Ok((rows.clone(), 6)));
        let swapped = [rows[1].clone(), rows[0].clone()];
        assert!(put_refs(&mut Vec::new(), 1, &swapped, 6).is_err());
    }

    fn entry(k0: u8, n: u8) -> IdemEntry {
        let mut key = [n; 16];
        key[0] = k0;
        IdemEntry {
            key,
            payload: [n ^ 0xFF; 16],
            ref_id: 0,
            branch_sym: 1,
            ref_seq: u32::from(n),
            flags: 1,
            append_hlc: u64::from(n) << 16,
            origin_lsn: 1_000 + u64::from(n),
        }
    }

    /// `IDEM` ([F11 §8], `11-runtime-tables.md:633-667`): capacity C = the smallest power of two ≥ max(16, 2n); the home
    /// slot is `u64le(key[0..8]) & (C − 1)`; insertion in ascending key order by linear probing. Keys with homes 15,
    /// 15, 0 and 5 land in slots 15, 0 (a collision that wraps), 1 (its home taken) and 5; every other slot is zero;
    /// `aux` = 4; each field sits at its offset; a lookup finds each key and misses an absent one, even through the
    /// wrapped run; two entries with one key are refused.
    #[test]
    fn idem_layer_is_a_hash_table_of_its_keys() {
        assert_eq!([0, 8, 9, 16, 17].map(idem_capacity), [16, 16, 32, 32, 64]);
        let es = [
            entry(0x35, 4),
            entry(0x20, 3),
            entry(0x1F, 2),
            entry(0x0F, 1),
        ];
        let mut b = Vec::new();
        put_idem(&mut b, &es).unwrap();
        let body = Body::parse(&b, 72).unwrap();
        assert_eq!((body.hdr.n_rows, body.hdr.aux, body.hdr.form), (16, 4, 2));
        let used: Vec<(usize, u8)> = decode_idem(&b)
            .unwrap()
            .iter()
            .map(|(s, e)| (*s, e.key[0]))
            .collect();
        assert_eq!(used, [(0, 0x1F), (1, 0x20), (5, 0x35), (15, 0x0F)]);
        let s5 = body.row(5);
        assert_eq!(&s5[0..16], &es[0].key);
        assert_eq!(&s5[16..32], &[4 ^ 0xFF; 16], "payload at 16");
        assert_eq!(&s5[32..40], &[0, 0, 0, 0, 1, 0, 0, 0], "ref_id, branch_sym");
        assert_eq!(
            &s5[40..48],
            &[4, 0, 0, 0, 1, 0, 0, 0],
            "ref_seq, flags used, reserved"
        );
        assert_eq!(&s5[48..56], &(4u64 << 16).to_le_bytes(), "append_hlc at 48");
        assert_eq!(&s5[56..64], &1_004u64.to_le_bytes(), "origin_lsn at 56");
        assert_eq!(&s5[64..72], &[0; 8], "no inline result");
        for s in [2, 3, 4, 6, 14] {
            assert!(body.row(s).iter().all(|&x| x == 0), "slot {s}");
        }
        for e in &es {
            assert_eq!(idem_lookup(&body, &e.key), Some(*e));
        }
        assert_eq!(
            idem_lookup(&body, &entry(0x2F, 9).key),
            None,
            "home 15, through the wrapped run"
        );
        assert_eq!(idem_lookup(&body, &entry(0x02, 9).key), None);
        assert!(put_idem(&mut Vec::new(), &[entry(0x0F, 1), entry(0x0F, 1)]).is_err());
    }

    /// `ALLOC` and `UIDX` ([F11 §9], `11-runtime-tables.md:669-714`): `ALLOC` is a dense layer from `aux` = the first
    /// `#N`, rows (`uid`, `ref_id`, `create_seq`) of 24 bytes; with no row `aux` is 0; a hole is refused. `UIDX` rows are
    /// (`uid`, `n`), 20 bytes, strictly ascending by `uid`.
    #[test]
    fn alloc_and_uidx_rows_follow_f11_9() {
        let rows = [
            AllocRow {
                uid: [0xC1; 16],
                ref_id: 0,
                create_seq: 101,
            },
            AllocRow {
                uid: [0x0A; 16],
                ref_id: 3,
                create_seq: 0x0102_0304,
            },
        ];
        let mut b = Vec::new();
        put_alloc(&mut b, form::LAYER, 51, &rows).unwrap();
        assert_eq!(&b[0..12], &[2, 0, 0, 0, 24, 0, 2, 0, 51, 0, 0, 0]);
        assert_eq!(&b[24..40], &[0xC1; 16]);
        assert_eq!(&b[40..48], &[0, 0, 0, 0, 101, 0, 0, 0]);
        assert_eq!(&b[64..72], &[3, 0, 0, 0, 4, 3, 2, 1]);
        assert_eq!(decode_alloc(&b), Ok((51, rows.to_vec())));
        let mut empty = Vec::new();
        put_alloc(&mut empty, form::LAYER, 0, &[]).unwrap();
        assert_eq!(
            empty,
            RtHdr {
                row_size: 24,
                form: 2,
                ..RtHdr::default()
            }
            .encode()
        );
        let hole = [AllocRow {
            create_seq: 0,
            ..rows[0]
        }];
        assert!(
            put_alloc(&mut Vec::new(), form::LAYER, 51, &hole)
                .unwrap_err()
                .contains("hole")
        );
        let u = [
            UidxRow {
                uid: [0x0A; 16],
                n: 52,
            },
            UidxRow {
                uid: [0xC1; 16],
                n: 51,
            },
        ];
        let mut b = Vec::new();
        put_uidx(&mut b, form::LAYER, &u).unwrap();
        assert_eq!(b.len(), 24 + 40);
        assert_eq!(
            &b[24..44],
            &[[0x0A; 16].as_slice(), &[52, 0, 0, 0]].concat()[..]
        );
        assert_eq!(decode_uidx(&b), Ok(u.to_vec()));
        assert!(put_uidx(&mut Vec::new(), form::LAYER, &[u[1], u[0]]).is_err());
    }

    /// `TREES`, `FILEOBS` and `FILES` ([F11 §12.4]–§12.5, `11-runtime-tables.md:826-914`; [F09 §14.4],
    /// `09-segments.md:998-1009`): a `TREES` row of 201 bytes with each field at its offset and its `root_text`, `sens`
    /// and `epochs` (32 B each) in the heap in that order; `FILEOBS` rows copied from their images, sorted by (`n`,
    /// `tree`), each `var` at its running heap offset; `FILES` rows of 56 bytes sorted by (`family`, `ref_id`,
    /// `file_no`). All read back; unsorted rows are refused.
    #[test]
    fn trees_fileobs_and_files_rows_follow_f11() {
        let mut root_id = [0u8; 57];
        root_id[0] = 1;
        root_id[1] = 0x77;
        let t = TreeRow {
            key: [0xE1; 16],
            os: 1,
            flags: 1,
            root_id,
            caps: [0; 16],
            last_head: [2; 33],
            last_settle_hlc: 0x0102_0304_0506_0708,
            dirty_count: 0,
            dirty_head: [0; 33],
            dirty_hlc: 0,
            root_text: "C:/w".into(),
            sens: vec![1, 1, b'a'],
            epochs: vec![
                Epoch {
                    scope_kind: 0,
                    scope_ref: 0,
                    digest: [0; 16],
                    hlc: 5,
                },
                Epoch {
                    scope_kind: 1,
                    scope_ref: 9,
                    digest: [0x0D; 16],
                    hlc: 6,
                },
            ],
        };
        let mut b = Vec::new();
        put_trees(&mut b, std::slice::from_ref(&t)).unwrap();
        let body = Body::parse(&b, 201).unwrap();
        assert_eq!(body.hdr.n_rows, 1, "one TREES row");
        let r = body.row(0);
        assert_eq!(
            (r[16], r[17], r[18], r[19]),
            (1, 1, 1, 0x77),
            "os, flags, root_id"
        );
        assert_eq!(&r[91..124], &[2; 33], "last_head at 91");
        assert_eq!(
            &r[124..132],
            &[8, 7, 6, 5, 4, 3, 2, 1],
            "last_settle_hlc at 124"
        );
        assert_eq!(
            [
                u32_le(r, 177),
                u32_le(r, 181),
                u32_le(r, 185),
                u32_le(r, 189),
                u32_le(r, 193),
                u32_le(r, 197)
            ],
            [0, 4, 4, 3, 7, 64]
        );
        let mut e1 = vec![1, 0, 0, 0, 9, 0, 0, 0];
        e1.extend([0x0D; 16]);
        e1.extend(6u64.to_le_bytes());
        assert_eq!(&body.heap[39..71], &e1[..], "the second epoch");
        assert_eq!(decode_trees(&b), Ok(vec![t.clone()]));
        let image = |n: u32, tree: u8| {
            FileObsRow {
                n,
                tree: [tree; 16],
                state: 1,
                resolver_version: 1,
                file_id: [0; 57],
                size: u64::from(n),
                times: [[0; 9]; 4],
                attrs: 0,
                last_oid: [0; 33],
                verified_at: 3,
                missing_since: 0,
            }
            .image()
            .to_vec()
        };
        let mut with_slice = image(4, 0xE2);
        with_slice[183..187].copy_from_slice(&3u32.to_le_bytes());
        with_slice.extend(b"xyz");
        let ims = [image(4, 0xE1), with_slice.clone(), image(9, 0xE0)];
        let refs: Vec<&[u8]> = ims.iter().map(Vec::as_slice).collect();
        let mut b = Vec::new();
        put_fileobs(&mut b, &refs).unwrap();
        let body = Body::parse(&b, 187).unwrap();
        assert_eq!(
            (body.hdr.n_rows, body.hdr.form, body.heap),
            (3, 2, &b"xyz"[..])
        );
        assert_eq!(&body.row(1)[179..187], &[0, 0, 0, 0, 3, 0, 0, 0]);
        assert_eq!(
            &body.row(2)[179..187],
            &[3, 0, 0, 0, 0, 0, 0, 0],
            "the running offset, len 0"
        );
        assert_eq!(decode_fileobs(&b), Ok(ims.to_vec()));
        assert!(put_fileobs(&mut Vec::new(), &[refs[1], refs[0]]).is_err());
        let files = [
            FileRow {
                family: 2,
                file_no: 3,
                from_lsn: 0x10,
                upto_lsn: 0x20,
                total_len: 0x30,
                digest16: [0x44; 16],
                ..FileRow::default()
            },
            FileRow {
                family: 6,
                file_no: 15,
                total_len: 0x0102,
                digest16: [0x55; 16],
                ..FileRow::default()
            },
        ];
        let mut b = Vec::new();
        put_files(&mut b, &files).unwrap();
        assert_eq!(&b[24..33], &[2, 0, 0, 0, 0, 3, 0, 0, 0], "FileRef");
        assert_eq!(&b[33..40], &[0; 7], "dest, algo, flags, reserved");
        assert_eq!(&b[40..48], &0x10u64.to_le_bytes(), "from_lsn at 16");
        assert_eq!(&b[48..56], &0x20u64.to_le_bytes(), "upto_lsn at 24");
        assert_eq!(&b[56..64], &0x30u64.to_le_bytes(), "total_len at 32");
        assert_eq!(&b[64..80], &[0x44; 16], "digest16 at 40");
        assert_eq!(decode_files(&b), Ok(files.to_vec()));
        assert!(put_files(&mut Vec::new(), &[files[1], files[0]]).is_err());
    }
}
