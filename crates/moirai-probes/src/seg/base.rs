//! The synthetic base segment `seg.base.1` at N nodes ([F09 §16.1]): every row of the probe's subset, its forward and
//! reverse CSR and its frozen bitsets, and [`Columns`], the reader of a segment's row sections that the probes, the
//! delta fold and the tests share.
//!
//! The base is the segment form of u10's [`crate::overlay::Base`], so a tail of [`crate::tail::synth`] applies to it:
//!
//! | Column | Value of row #N | Source |
//! |---|---|---|
//! | `NODE` | kind 1, status #N mod 4, resolution 0; byte 3 `_reserved`, 0; criticality 2, confidence 0, authority 4 (the defaults); no flag, parent or counter; `rev_seq`, `created_tx`, `updated_tx` the base's `base_seq`; `last_op_lsn` the notional record just below the tail (`upto_lsn − 1`, as u10's `prev`); `title_off`, `fields_off` `NONE32`, `body_ref` 0 | [F08 §3.1]; the probe's choice beyond the overlay's base |
//! | `CREATOR` | actor 5, role 2: the tail's creator symbols | [F08 §4] |
//! | `TOPO` | #N, distinct (the base has no precedence order to keep) | [F09 §5.4] |
//! | `DEFER`, `DUE` | 0; `due` 1,790,000,000 + #N on every fourth row | [F08 §4] |
//! | `PRIO` | the default `priority`, 2 ([`DEFAULT_PRIORITY`]); in [`build_with`]'s base, its function of #N | [F08 §4], [F09 §5.4] (R-66) |
//! | adjacency | 3 out-edges per node, kinds 1–4 (u10's edge codes), destinations drawn from a hash of (#N, j), stored twice: the out-lists and their reverse | [AR §8.1] ("3 edges/node stored twice"); [F09 §7.1] |
//! | bitsets | family 1 kind 1 (every row) and family 2 (kind 1, status s) for s = 0–3 | [F09 §9.2]; the overlay's families |
//!
//! Its hot index (`NODE`, the CSR, `BMDIR` and the bitsets) is ≈ 99 B per node: 60 + 8 + 30 + ≈ 0.6 ([AR §8.1]); the
//! cold columns (`CREATOR`, `TOPO`, `DEFER`, `DUE` and `PRIO`) add 19 B.
//!
//! [`build_with`] builds the same base with each row's `priority` from a function ([`ready_priority`]: P0–P4, the
//! `priority` column of [F08 §4]): a base whose priorities are not all the default, which the tests of the delta's
//! fold and the pins use (R-86 ruling 6). [`build`] keeps the default 2, so every other base keeps its bytes.
//!
//! [`build_full`] builds the full checkpoint's base (u21 cut §3.2, D-4): [`build`]'s sections, each row's `title_off`
//! and `fields_off` pointing into the base's own pools, and ten more sections that a fold reads: `UID`, `TITLE_BLOB`,
//! `FIELDS_BLOB`, an empty `BLOBTAB`, `DOCLEN` and `FTSSTAT` (both `derived-optional`, [F09 §3.3]), and the snapshots
//! `FILES`, `REFS`, `ALLOC` and `TREES` ([F11 §1.3]); its titles and fields come from [`Vocab`], and its `tok_ver` is 1
//! ([F09 §12.4]). It carries no `TERMS` or `POST` (u21 cut F-17), so it is not valid under V-7 either.

use super::bitset::{Frozen, encode_frozen};
use super::blobs::BLOB_REF_LEN;
use super::rt::{self, FileRow, RefRow, TreeRow, put_files, put_refs, put_trees};
use super::text::{self, FtsStat, put_doclen, put_ftsstat, tokenize};
use super::{
    CREATOR_LEN, Creator, Header, NODE_HDR_LEN, NONE32, NodeHdr, SegView, Writer, fixed_rows,
    put_fixed, put_u32s, seg_kind, tag, u32_at,
};
use crate::overlay::{Base, BitsetKey, kind_key, kind_status_key};
use crate::tail::enc::{put_uvar, put_vbytes, uvar_len};
use crate::tail::rt::Epoch;
use crate::tail::synth::{BASE_KIND, EKINDS, STATUSES};

/// Out-edges per base node ([AR §8.1]).
pub const OUT_EDGES: usize = 3;
/// The creator actor of every base node (the tail's `c_actor`).
pub const ACTOR: u32 = 5;
/// The creator role (the tail's `c_role`).
pub const ROLE: u16 = 2;
/// The first `due` value, Unix seconds.
pub const DUE_FROM: u32 = 1_790_000_000;
/// The smallest base: 3 distinct out-edges per node need room.
pub const MIN_N: u32 = 8;
/// `BMDIR`'s row width ([F09 §9.2]).
pub const BMDIR_ROW: u32 = 4;

/// What a base is built from.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Spec {
    /// N: rows #1..=N.
    pub n: u32,
    /// `upto_lsn`: the tail's first lsn ([F09 §2.3]).
    pub upto_lsn: u64,
    /// `base_seq`: the largest `seq` below `upto_lsn` ([F09 §2.3]).
    pub base_seq: u64,
}

fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Spec {
    /// The overlay's model of this base.
    pub fn base(&self) -> Base {
        Base {
            n: self.n,
            kind: BASE_KIND,
            statuses: STATUSES,
        }
    }

    /// Row #`id`'s header.
    pub fn row(&self, id: u32) -> NodeHdr {
        NodeHdr {
            kind: BASE_KIND,
            status: self.base().status(id),
            resolution: 0,
            criticality: 2,
            confidence: 0,
            authority: 4,
            flags: 0,
            rev_seq: self.base_seq,
            parent: 0,
            created_tx: self.base_seq as u32,
            updated_tx: self.base_seq as u32,
            last_op_lsn: self.upto_lsn.saturating_sub(1),
            title_off: NONE32,
            fields_off: NONE32,
            ..NodeHdr::default()
        }
    }

    /// Row #`id`'s `CREATOR`.
    pub fn creator(&self, _id: u32) -> Creator {
        Creator {
            actor: ACTOR,
            role: ROLE,
        }
    }

    /// Row #`id`'s `topo`.
    pub fn topo(&self, id: u32) -> u32 {
        id
    }

    /// Row #`id`'s `defer_until`.
    pub fn defer(&self, _id: u32) -> u32 {
        0
    }

    /// Row #`id`'s `due`.
    pub fn due(&self, id: u32) -> u32 {
        if id.is_multiple_of(4) {
            DUE_FROM + id
        } else {
            0
        }
    }

    /// Row #`id`'s out-list: [`OUT_EDGES`] distinct `(kind, dst)`, no self-edge, sorted by `(kind, dst)`
    /// ([F09 §7.1]).
    pub fn out_edges(&self, id: u32) -> Vec<(u8, u32)> {
        let n = u64::from(self.n);
        let mut out: Vec<(u8, u32)> = Vec::with_capacity(OUT_EDGES);
        for j in 0..OUT_EDGES as u64 {
            let h = mix(u64::from(id) << 8 | j);
            let kind = 1 + (h % u64::from(EKINDS)) as u8;
            let mut dst = 1 + ((h >> 16) % n) as u32;
            while dst == id || out.contains(&(kind, dst)) {
                dst = dst % self.n + 1;
            }
            out.push((kind, dst));
        }
        out.sort_unstable();
        out
    }

    /// The header ([F09 §2.3], seg-base column).
    pub fn header(&self) -> Header {
        Header {
            seg_kind: seg_kind::BASE,
            n_rows: self.n,
            file_no: 1,
            base_seq: self.base_seq,
            upto_lsn: self.upto_lsn,
            rt_upto_lsn: self.upto_lsn,
            ..Header::default()
        }
    }

    /// The non-empty bitsets in `BMDIR` order: (key, members).
    pub fn bitsets(&self) -> Vec<(BitsetKey, Vec<u32>)> {
        let mut v = vec![(kind_key(BASE_KIND), (1..=self.n).collect())];
        for s in 0..STATUSES {
            let m: Vec<u32> = (1..=self.n)
                .filter(|&id| self.base().status(id) == s)
                .collect();
            if !m.is_empty() {
                v.push((kind_status_key(BASE_KIND, s), m));
            }
        }
        v
    }
}

/// Adjacency in CSR form: `off` (rows + 1), `ids`, `kinds` ([F09 §7.1]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Csr {
    /// Offsets.
    pub off: Vec<u32>,
    /// `#N`s.
    pub ids: Vec<u32>,
    /// Edge kinds.
    pub kinds: Vec<u8>,
}

impl Csr {
    /// The CSR of `lists`, each already sorted by `(kind, id)`: owned lists or borrowed slices, so a writer need not
    /// copy its rows' lists.
    pub fn of_lists<L: AsRef<[(u8, u32)]>, I: IntoIterator<Item = L>>(lists: I) -> Csr {
        let mut c = Csr {
            off: vec![0],
            ..Csr::default()
        };
        for l in lists {
            for &(k, id) in l.as_ref() {
                c.kinds.push(k);
                c.ids.push(id);
            }
            c.off.push(c.ids.len() as u32);
        }
        c
    }

    /// The reverse of a base's forward CSR (rows #1..=n): row d's in-list holds `(k, s)` for every `(k, d)` in s's
    /// out-list, sorted by `(kind, src)` (I-P3, [F09 §7.1]).
    pub fn reverse(&self, n: u32) -> Csr {
        let mut count = vec![0u32; n as usize + 1];
        for &d in &self.ids {
            count[d as usize] += 1;
        }
        let mut off = vec![0u32; n as usize + 1];
        for d in 1..=n as usize {
            off[d] = off[d - 1] + count[d];
        }
        let mut fill = off.clone();
        let mut pairs = vec![(0u8, 0u32); self.ids.len()];
        for s in 0..n as usize {
            for e in self.off[s] as usize..self.off[s + 1] as usize {
                let d = self.ids[e] as usize;
                pairs[fill[d - 1] as usize] = (self.kinds[e], s as u32 + 1);
                fill[d - 1] += 1;
            }
        }
        for d in 0..n as usize {
            pairs[off[d] as usize..off[d + 1] as usize].sort_unstable();
        }
        Csr {
            off,
            ids: pairs.iter().map(|p| p.1).collect(),
            kinds: pairs.iter().map(|p| p.0).collect(),
        }
    }
}

/// Writes one CSR direction: the offsets, the `#N`s and the kinds under `tags` ([F09 §7.1]).
pub fn put_csr(w: &mut Writer, c: &Csr, tags: [u16; 3]) -> Result<(), String> {
    let e = c.ids.len() as u32;
    w.push(tags[0], c.off.len() as u32, |o| {
        put_u32s(o, &c.off);
        Ok(())
    })?;
    w.push(tags[1], e, |o| {
        put_u32s(o, &c.ids);
        Ok(())
    })?;
    w.push(tags[2], e, |o| {
        o.extend_from_slice(&c.kinds);
        Ok(())
    })
}

/// Writes `BMDIR` and one section per bitset: frozen (`body`'s bytes come from `put`).
pub fn put_bitsets(
    w: &mut Writer,
    sets: &[(BitsetKey, u32)],
    put: impl FnMut(usize, &mut Vec<u8>) -> Result<(), String>,
) -> Result<(), String> {
    put_bmdir(w, sets)?;
    put_bm(w, sets, put)
}

/// Writes `BMDIR`: the keys of `sets` ([F09 §9.2]). A segment with sections between `BMDIR` and
/// `BM.<0>` in tag order writes them between this and [`put_bm`].
pub fn put_bmdir(w: &mut Writer, sets: &[(BitsetKey, u32)]) -> Result<(), String> {
    w.push(tag::BMDIR, sets.len() as u32, |o| {
        let rows: Vec<&[u8]> = sets.iter().map(|(k, _)| &k[..]).collect();
        put_fixed(o, BMDIR_ROW, &rows);
        Ok(())
    })
}

/// Writes one section per bitset of `sets`, `BM.<i>`, its bytes from `put`.
pub fn put_bm(
    w: &mut Writer,
    sets: &[(BitsetKey, u32)],
    mut put: impl FnMut(usize, &mut Vec<u8>) -> Result<(), String>,
) -> Result<(), String> {
    for (i, (_, count)) in sets.iter().enumerate() {
        w.push(tag::bm(i as u16), *count, |o| put(i, o))?;
    }
    Ok(())
}

/// The row sections a writer puts first, in tag order: `NODE`, `CREATOR`, `TOPO`, `DEFER`, `DUE`. `PRIO` follows
/// ([`put_prio`]): right after these in a subset segment, after `UID` in a full one (P-1).
pub fn put_rows(
    w: &mut Writer,
    nodes: &[NodeHdr],
    creators: &[Creator],
    cold: [&[u32]; 3],
) -> Result<(), String> {
    let n = nodes.len() as u32;
    w.push(tag::NODE, n, |o| {
        o.reserve(NODE_HDR_LEN * nodes.len());
        for h in nodes {
            o.extend_from_slice(&h.encode());
        }
        Ok(())
    })?;
    w.push(tag::CREATOR, n, |o| {
        o.reserve(CREATOR_LEN * creators.len());
        for c in creators {
            c.put(o);
        }
        Ok(())
    })?;
    for (t, col) in [tag::TOPO, tag::DEFER, tag::DUE].into_iter().zip(cold) {
        w.push(t, n, |o| {
            put_u32s(o, col);
            Ok(())
        })?;
    }
    Ok(())
}

/// Writes `PRIO` ([F09 §5.4]): one `u8` per row, `count` = the rows (R-66).
pub fn put_prio(w: &mut Writer, prios: &[u8]) -> Result<(), String> {
    w.push(tag::PRIO, prios.len() as u32, |o| {
        o.extend_from_slice(prios);
        Ok(())
    })
}

/// The sections of a segment with `bitsets` bitsets: 6 row columns (`PRIO` among them), 6 CSR columns, `BMDIR` and the
/// bitsets, plus `IDS` in an upper segment.
pub const fn sections(bitsets: usize, upper: bool) -> usize {
    6 + 6 + 1 + bitsets + upper as usize
}

/// The forward CSR of a base.
pub fn out_csr(spec: &Spec) -> Csr {
    Csr::of_lists((1..=spec.n).map(|id| spec.out_edges(id)))
}

/// The default `priority`, P2 ([F08 §4]).
pub const DEFAULT_PRIORITY: u8 = 2;

/// Builds the base segment of `spec`, every row's `priority` the default 2.
pub fn build(spec: &Spec) -> Result<Vec<u8>, String> {
    build_with(spec, |_| DEFAULT_PRIORITY)
}

/// The salt of the non-default priorities (u20 cut D-12, the retired `ready` probe's).
pub const SALT_P: u64 = 0x6D31_345F_7072_696F;

/// The non-default priority of row #`id` (u20 cut D-12): `mix(SALT_P ^ #N) mod 5`, P0–P4 ([F08 §4]).
pub fn ready_priority(id: u32) -> u8 {
    (mix(SALT_P ^ u64::from(id)) % 5) as u8
}

/// Builds the base segment of `spec` with row #N's `priority` = `prio(#N)` in `PRIO` (u20 cut D-12; R-66); every other
/// byte is [`build`]'s.
pub fn build_with(spec: &Spec, prio: impl Fn(u32) -> u8) -> Result<Vec<u8>, String> {
    if spec.n < MIN_N {
        return Err(format!("a base has at least {MIN_N} nodes"));
    }
    let ids = 1..=spec.n;
    let nodes: Vec<NodeHdr> = ids.clone().map(|id| spec.row(id)).collect();
    let prios: Vec<u8> = ids.clone().map(prio).collect();
    let creators: Vec<Creator> = ids.clone().map(|id| spec.creator(id)).collect();
    let topo: Vec<u32> = ids.clone().map(|id| spec.topo(id)).collect();
    let defer: Vec<u32> = ids.clone().map(|id| spec.defer(id)).collect();
    let due: Vec<u32> = ids.map(|id| spec.due(id)).collect();
    let out = out_csr(spec);
    let inn = out.reverse(spec.n);
    let sets = spec.bitsets();
    let n = spec.n as usize;
    let mut w = Writer::new(sections(sets.len(), false), 121 * n);
    put_rows(&mut w, &nodes, &creators, [&topo, &defer, &due])?;
    put_prio(&mut w, &prios)?;
    put_csr(&mut w, &out, [tag::OUT_OFF, tag::OUT_DST, tag::OUT_KIND])?;
    put_csr(&mut w, &inn, [tag::IN_OFF, tag::IN_SRC, tag::IN_KIND])?;
    let dir: Vec<(BitsetKey, u32)> = sets.iter().map(|(k, m)| (*k, m.len() as u32)).collect();
    put_bitsets(&mut w, &dir, |i, o| {
        encode_frozen(&sets[i].1, o).map(|_| ())
    })?;
    w.finish(&spec.header())
}

/// The salt of the full base's uids (u21 cut §3.2).
pub const SALT_U: u64 = 0x6D31_345F_7569_6431;
/// The salt of its vocabulary.
pub const SALT_W: u64 = 0x6D31_345F_776F_7264;
/// The salt of its titles.
pub const SALT_T: u64 = 0x6D31_345F_7469_746C;
/// The salt of its fields.
pub const SALT_F: u64 = 0x6D31_345F_6669_656C;
/// The words of its vocabulary.
pub const VOCAB: u32 = 4_096;
/// A title's bytes ([AR §8.1]).
pub const TITLE_LEN: usize = 60;
/// A field value's bytes ([AR §8.1]).
pub const FIELD_LEN: usize = 24;
/// The field the probe stores as a row's title (the probe's convention, [F08 §7.1]; u20 cut §3.9).
pub const TITLE_FIELD: u32 = 1;
/// The field the probe takes as a document's abstract ([F08 §9]; [F09 §12.1]).
pub const ABSTRACT_FIELD: u32 = 2;
/// The refs of the full base: `main` and 49 lanes ([AR §8.1], "~50 live refs worst case").
pub const REFS: u32 = 50;
/// The `hist` files the full base's `FILES` names (u20 cut F-8).
pub const HIST_FILES: std::ops::RangeInclusive<u32> = 2..=9;
/// The `blobs` files it names.
pub const BLOBS_FILES: std::ops::RangeInclusive<u32> = 10..=13;
/// The `hlc` of the full base's trees' settle: below the generator's first commit's (`tail/synth.rs`).
pub const BASE_SETTLE_HLC: u64 = 1_789_999_999_000 << 16;

/// The full base's vocabulary: word i is 3–9 lowercase letters drawn from `mix(SALT_W ^ i)`.
#[derive(Clone, Debug)]
pub struct Vocab(Vec<Vec<u8>>);

impl Default for Vocab {
    fn default() -> Vocab {
        Vocab::new()
    }
}

impl Vocab {
    /// The [`VOCAB`] words.
    pub fn new() -> Vocab {
        Vocab(
            (0..VOCAB)
                .map(|i| {
                    let h = mix(SALT_W ^ u64::from(i));
                    let len = 3 + (h % 7) as usize;
                    (0..len)
                        .map(|k| b'a' + (((h >> (3 + 5 * k)) & 31) % 26) as u8)
                        .collect()
                })
                .collect(),
        )
    }

    /// Word `i`.
    pub fn word(&self, i: u32) -> &[u8] {
        &self.0[i as usize]
    }

    /// `size` bytes of text: words drawn by `mix(seed ^ k)` mod [`VOCAB`] for k = 0, 1, …, separated by one space, cut
    /// to `size`.
    pub fn text(&self, seed: u64, size: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(size + 10);
        let mut k = 0u64;
        while out.len() < size {
            if !out.is_empty() {
                out.push(b' ');
            }
            out.extend_from_slice(&self.0[(mix(seed ^ k) % u64::from(VOCAB)) as usize]);
            k += 1;
        }
        out.truncate(size);
        out
    }

    /// Row #`id`'s title in the full base: 60 bytes.
    pub fn title(&self, id: u32) -> Vec<u8> {
        self.text(SALT_T ^ u64::from(id), TITLE_LEN)
    }

    /// Row #`id`'s stored fields in the full base, by name: the abstract (field 2) and 1–3 of the fields 3–8, chosen by
    /// `mix(SALT_F ^ #N)`, each a 24-byte text.
    pub fn fields(&self, id: u32) -> Vec<(u32, Vec<u8>)> {
        let h = mix(SALT_F ^ u64::from(id));
        let k = 1 + (h % 3) as u32;
        let first = ((h >> 8) % 6) as u32;
        let mut names: Vec<u32> = (0..k).map(|j| 3 + (first + j) % 6).collect();
        names.sort_unstable();
        let at = |name: u32| SALT_F ^ (u64::from(id) << 8 | u64::from(name));
        let mut v = vec![(ABSTRACT_FIELD, self.text(at(ABSTRACT_FIELD), FIELD_LEN))];
        v.extend(
            names
                .into_iter()
                .map(|name| (name, self.text(at(name), FIELD_LEN))),
        );
        v
    }
}

/// The typed value of a text ([F08 §5.1]–§5.2: type byte 6, then a `vstr`), as a field entry stores it.
pub fn text_value(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + s.len());
    v.push(crate::tail::commit::ty::TEXT);
    put_vbytes(&mut v, s);
    v
}

/// The [F08 §6.1] field block of `fields` (name, typed value bytes), already by name ascending: `n`, then each entry's
/// `field_sym` and its typed value.
pub fn field_block(fields: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = Vec::with_capacity(1 + fields.iter().map(|f| 2 + f.1.len()).sum::<usize>());
    put_uvar(&mut b, fields.len() as u64);
    for (name, v) in fields {
        put_uvar(&mut b, u64::from(*name));
        b.extend_from_slice(v);
    }
    b
}

/// Row #`id`'s field block in the full base.
pub fn full_block(vocab: &Vocab, id: u32) -> Vec<u8> {
    let f: Vec<(u32, Vec<u8>)> = vocab
        .fields(id)
        .into_iter()
        .map(|(n, t)| (n, text_value(&t)))
        .collect();
    field_block(&f)
}

/// Row #`id`'s uid in the full base: `mix(SALT_U ^ #N)` then `mix(SALT_U ^ #N ^ 0xFF…)`, little-endian.
pub fn full_uid(id: u32) -> [u8; 16] {
    let x = SALT_U ^ u64::from(id);
    let mut u = [0u8; 16];
    u[..8].copy_from_slice(&mix(x).to_le_bytes());
    u[8..].copy_from_slice(&mix(x ^ u64::MAX).to_le_bytes());
    u
}

/// The `id16` of `main`'s tip in the full base: the commit just below the tail.
pub fn full_tip() -> [u8; 16] {
    full_uid(0)
}

/// The full base's `REFS` (u20 cut §3.9; u21 cut §3.2): `main` (`ref_id` 0, kind 1 `work`) at the commit just below
/// the tail, `seq` = `base_seq`, and `lane/l01` … `lane/l49` (kind 1) forked from it with no commit of their own, by
/// [F11 §3.6]'s fork rule with no pinned set; `next_ref_id` 50.
pub fn full_refs(spec: &Spec) -> Vec<RefRow> {
    let ops = 4 * spec.base_seq;
    let bytes = 1_000 * spec.base_seq;
    let tip_lsn = spec.upto_lsn.saturating_sub(1);
    let seq32 = spec.base_seq as u32;
    let main = RefRow {
        ref_id: 0,
        kind: 1,
        tip: full_tip(),
        tip_lsn,
        gen_no: seq32,
        ref_seq_next: seq32 + 1,
        ops_since_fork: ops as u32,
        overlay_ops: ops as u32,
        overlay_bytes: bytes as u32,
        ops_total: ops,
        bytes_total: bytes,
        name: "main".into(),
        absorbed: vec![(0, seq32)],
        moves: vec![tip_lsn],
        ..RefRow::default()
    };
    let mut v = vec![main];
    for i in 1..REFS {
        v.push(RefRow {
            ref_id: i,
            kind: 1,
            tip: full_tip(),
            tip_lsn,
            gen_no: seq32,
            ref_seq_next: 1,
            fork_commit: full_tip(),
            fork_lsn: tip_lsn,
            fork_seq: spec.base_seq,
            fork_ref_id: 0,
            overlay_ops: ops as u32,
            overlay_bytes: bytes as u32,
            trunk_mark_ops: ops,
            trunk_mark_bytes: bytes,
            name: format!("lane/l{i:02}"),
            absorbed: vec![(0, seq32)],
            ..RefRow::default()
        });
    }
    v
}

/// The full base's `TREES`: the generator's [`crate::tail::synth::TREES`] trees, keys `[0xE0 + i; 16]`
/// (`tail/synth.rs`), `os` 1, `first_settle_done`, an NTFS root id, each with one `full-tree` epoch at
/// [`BASE_SETTLE_HLC`].
pub fn full_trees() -> Vec<TreeRow> {
    (0..crate::tail::synth::TREES)
        .map(|i| {
            let mut root_id = [0u8; 57];
            root_id[0] = 1;
            root_id[1..17].copy_from_slice(&full_uid(0xE000 + u32::from(i)));
            root_id[17..33].copy_from_slice(&full_uid(0xE100 + u32::from(i)));
            let mut last_head = [0u8; 33];
            last_head[0] = 1;
            last_head[1..17].copy_from_slice(&full_uid(0xE200 + u32::from(i)));
            TreeRow {
                key: [0xE0 + i; 16],
                os: 1,
                flags: 1,
                root_id,
                caps: [0; 16],
                last_head,
                last_settle_hlc: BASE_SETTLE_HLC,
                dirty_count: 0,
                dirty_head: [0; 33],
                dirty_hlc: 0,
                root_text: format!("C:/probe/tree{i}"),
                sens: Vec::new(),
                epochs: vec![Epoch {
                    scope_kind: 0,
                    scope_ref: 0,
                    digest: [0; 16],
                    hlc: BASE_SETTLE_HLC,
                }],
            }
        })
        .collect()
}

/// The full base's `FILES` (u20 cut F-8): `hist.2` … `hist.9`, which cover `[0, upto_lsn)` in eighths, and
/// `blobs.10` … `blobs.13`; lengths and digests are the probe's.
pub fn full_files(spec: &Spec) -> Vec<FileRow> {
    let step = spec.upto_lsn / 8;
    let digest = |k: u32| full_uid(0xF000 + k);
    let mut v: Vec<FileRow> = HIST_FILES
        .map(|k| FileRow {
            family: 2,
            file_no: k,
            from_lsn: u64::from(k - 2) * step,
            upto_lsn: u64::from(k - 1) * step,
            total_len: 4_096 * u64::from(k),
            digest16: digest(k),
            ..FileRow::default()
        })
        .collect();
    v.extend(BLOBS_FILES.map(|k| FileRow {
        family: 6,
        file_no: k,
        total_len: 65_536 * u64::from(k),
        digest16: digest(k),
        ..FileRow::default()
    }));
    v
}

/// A text's token count for `DOCLEN`, saturating at 65,535 ([F09 §12.4]).
fn tokens(s: &[u8]) -> Result<u16, String> {
    Ok(u16::try_from(tokenize(s, |_| {})?).unwrap_or(u16::MAX))
}

/// The sections of a full base with `bitsets` bitsets: [`sections`]'s 13 + k, `UID`, `TITLE_BLOB`, `FIELDS_BLOB`,
/// `BLOBTAB`, `DOCLEN`, `FTSSTAT`, `FILES`, `REFS`, `ALLOC` and `TREES`.
pub const fn full_sections(bitsets: usize) -> usize {
    sections(bitsets, false) + 10
}

/// Builds the full base of `spec` (u21 cut §3.2, D-4): everything [`build`] writes, each row's `title_off` and
/// `fields_off` pointing into the base's own pools, and the sections a fold reads to give a touched row its complete
/// state and the store-level snapshots it starts from: `UID`, `TITLE_BLOB`, `FIELDS_BLOB`, an empty `BLOBTAB`,
/// `DOCLEN`, `FTSSTAT`, `FILES`, `REFS`, `ALLOC` and `TREES`. `tok_ver` is 1. No `TERMS` or `POST` (u21 cut F-17): the
/// checkpoint never reads them, and the base stays invalid under V-7 ([F09 §17.1]), as [`build`]'s is.
pub fn build_full(spec: &Spec) -> Result<Vec<u8>, String> {
    if spec.n < MIN_N {
        return Err(format!("a base has at least {MIN_N} nodes"));
    }
    let vocab = Vocab::new();
    let n = spec.n as usize;
    let ids = 1..=spec.n;
    // Pass 1: each row's pool offsets and DOCLEN; the pools themselves are written in the sections' closures.
    let mut nodes = Vec::with_capacity(n);
    let mut doclen = Vec::with_capacity(n);
    let (mut toff, mut foff) = (0u64, 0u64);
    for id in ids.clone() {
        let title = vocab.title(id);
        let fields = vocab.fields(id);
        let typed: Vec<(u32, Vec<u8>)> = fields.iter().map(|(n, t)| (*n, text_value(t))).collect();
        let block = field_block(&typed);
        nodes.push(NodeHdr {
            title_off: u32::try_from(toff).map_err(|_| "TITLE_BLOB past 4 GiB")?,
            fields_off: u32::try_from(foff).map_err(|_| "FIELDS_BLOB past 4 GiB")?,
            ..spec.row(id)
        });
        toff += (uvar_len(title.len() as u64) + title.len()) as u64;
        foff += (uvar_len(block.len() as u64) + block.len()) as u64;
        doclen.push([tokens(&title)?, tokens(&fields[0].1)?, 0]);
    }
    let mut stat = [FtsStat::default(); 3];
    for d in &doclen {
        for f in 0..3 {
            if d[f] > 0 {
                stat[f].docs += 1;
                stat[f].total_len += u64::from(d[f]);
            }
        }
    }
    let creators: Vec<Creator> = ids.clone().map(|id| spec.creator(id)).collect();
    let topo: Vec<u32> = ids.clone().map(|id| spec.topo(id)).collect();
    let defer: Vec<u32> = ids.clone().map(|id| spec.defer(id)).collect();
    let due: Vec<u32> = ids.clone().map(|id| spec.due(id)).collect();
    let mut uids: Vec<([u8; 16], u32)> = ids.clone().map(|id| (full_uid(id), id)).collect();
    uids.sort_unstable();
    let out = out_csr(spec);
    let inn = out.reverse(spec.n);
    let sets = spec.bitsets();
    let dir: Vec<(BitsetKey, u32)> = sets.iter().map(|(k, m)| (*k, m.len() as u32)).collect();
    let mut w = Writer::new(full_sections(sets.len()), 330 * n);
    put_rows(&mut w, &nodes, &creators, [&topo, &defer, &due])?;
    drop((nodes, creators, topo, defer, due));
    w.push(tag::UID, spec.n, |o| {
        o.reserve(8 + 20 * n);
        o.extend_from_slice(&spec.n.to_le_bytes());
        o.extend_from_slice(&20u32.to_le_bytes());
        for (uid, id) in &uids {
            o.extend_from_slice(uid);
            o.extend_from_slice(&id.to_le_bytes());
        }
        Ok(())
    })?;
    drop(uids);
    put_prio(&mut w, &vec![DEFAULT_PRIORITY; n])?;
    w.push(tag::TITLE_BLOB, spec.n, |o| {
        for id in ids.clone() {
            put_vbytes(o, &vocab.title(id));
        }
        Ok(())
    })?;
    w.push(tag::FIELDS_BLOB, spec.n, |o| {
        for id in ids.clone() {
            put_vbytes(o, &full_block(&vocab, id));
        }
        Ok(())
    })?;
    w.push(tag::BLOBTAB, 0, |o| {
        put_fixed(o, BLOB_REF_LEN as u32, &[]);
        Ok(())
    })?;
    put_csr(&mut w, &out, [tag::OUT_OFF, tag::OUT_DST, tag::OUT_KIND])?;
    put_csr(&mut w, &inn, [tag::IN_OFF, tag::IN_SRC, tag::IN_KIND])?;
    drop((out, inn));
    put_bmdir(&mut w, &dir)?;
    w.push(tag::DOCLEN, spec.n, |o| {
        put_doclen(o, &doclen);
        Ok(())
    })?;
    w.push(tag::FTSSTAT, 3, |o| {
        put_ftsstat(o, &stat);
        Ok(())
    })?;
    let files = full_files(spec);
    w.push(tag::FILES, files.len() as u32, |o| put_files(o, &files))?;
    let refs = full_refs(spec);
    w.push(tag::REFS, refs.len() as u32, |o| {
        put_refs(o, rt::form::SNAPSHOT, &refs, REFS)
    })?;
    w.push(tag::ALLOC, spec.n, |o| {
        let mut rows = Vec::with_capacity(24 * n);
        for id in ids.clone() {
            rows.extend_from_slice(&full_uid(id));
            rows.extend_from_slice(&0u32.to_le_bytes());
            rows.extend_from_slice(&1u32.to_le_bytes());
        }
        rt::put_body(o, 24, rt::form::SNAPSHOT, 1, &rows, &[], &[])
    })?;
    let trees = full_trees();
    w.push(tag::TREES, trees.len() as u32, |o| put_trees(o, &trees))?;
    put_bm(&mut w, &dir, |i, o| {
        encode_frozen(&sets[i].1, o).map(|_| ())
    })?;
    w.finish(&Header {
        tok_ver: text::TOK_VER,
        ..spec.header()
    })
}

/// The row sections of a parsed segment, row `i` being #N = i + 1 in a base and `IDS[i]` in an upper segment
/// ([F09 §2.4]).
#[derive(Clone, Debug)]
pub struct Columns<'a> {
    /// The segment.
    pub view: SegView<'a>,
    /// Rows.
    pub rows: usize,
    /// `IDS` (an upper segment), else empty.
    pub ids: &'a [u8],
    /// `NODE`.
    pub node: &'a [u8],
    /// `CREATOR`.
    pub creator: &'a [u8],
    /// `TOPO`.
    pub topo: &'a [u8],
    /// `DEFER`.
    pub defer: &'a [u8],
    /// `DUE`.
    pub due: &'a [u8],
    /// `PRIO`, required ([F09 §3.1]: R in every segment kind).
    pub prio: &'a [u8],
    /// `OUT_OFF`, `OUT_DST`, `OUT_KIND`.
    pub out: [&'a [u8]; 3],
    /// `IN_OFF`, `IN_SRC`, `IN_KIND`.
    pub inn: [&'a [u8]; 3],
    /// `BMDIR`'s keys.
    pub keys: Vec<BitsetKey>,
}

impl<'a> Columns<'a> {
    /// Parses `bytes` and checks every column's length against the row count ([F09 §4.1], V-11).
    pub fn parse(bytes: &'a [u8]) -> Result<Columns<'a>, String> {
        let view = SegView::parse(bytes)?;
        let rows = view.header.n_rows as usize;
        let ids = view.section(tag::IDS).unwrap_or(&[]);
        let get = |t: u16, w: usize| -> Result<&'a [u8], String> {
            let s = view.need(t)?;
            if s.len() != w * rows || view.entry(t).map(|e| e.count as usize) != Some(rows) {
                return Err(format!(
                    "section {t:#06x} does not hold {rows} rows of {w} B"
                ));
            }
            Ok(s)
        };
        let node = get(tag::NODE, NODE_HDR_LEN)?;
        let creator = get(tag::CREATOR, CREATOR_LEN)?;
        let (topo, defer, due) = (get(tag::TOPO, 4)?, get(tag::DEFER, 4)?, get(tag::DUE, 4)?);
        let prio = get(tag::PRIO, 1)?;
        let csr = |t: [u16; 3]| -> Result<[&'a [u8]; 3], String> {
            let off = view.need(t[0])?;
            if off.len() != 4 * (rows + 1) || u32_at(off, 0) != 0 {
                return Err(format!(
                    "section {:#06x} does not hold rows + 1 offsets",
                    t[0]
                ));
            }
            let e = u32_at(off, rows) as usize;
            let (dst, kind) = (view.need(t[1])?, view.need(t[2])?);
            if dst.len() != 4 * e
                || kind.len() != e
                || (1..=rows).any(|i| u32_at(off, i) < u32_at(off, i - 1))
            {
                return Err(format!("the CSR at {:#06x} breaks its offsets", t[0]));
            }
            Ok([off, dst, kind])
        };
        let out = csr([tag::OUT_OFF, tag::OUT_DST, tag::OUT_KIND])?;
        let inn = csr([tag::IN_OFF, tag::IN_SRC, tag::IN_KIND])?;
        let keys = match view.section(tag::BMDIR) {
            Some(b) => fixed_rows(b, BMDIR_ROW)?
                .into_iter()
                .map(|r| r.try_into().expect("4 bytes"))
                .collect(),
            None => Vec::new(),
        };
        if !ids.is_empty() && ids.len() != 4 * rows {
            return Err("IDS does not hold n_rows ids".into());
        }
        Ok(Columns {
            view,
            rows,
            ids,
            node,
            creator,
            topo,
            defer,
            due,
            prio,
            out,
            inn,
            keys,
        })
    }

    /// Row `i`'s header.
    pub fn node(&self, i: usize) -> NodeHdr {
        NodeHdr::row(self.node, i)
    }

    /// Row `i`'s `CREATOR`.
    pub fn creator(&self, i: usize) -> Creator {
        Creator::at(self.creator, i)
    }

    /// Row `i`'s `priority`, from `PRIO`.
    pub fn prio(&self, i: usize) -> u8 {
        self.prio[i]
    }

    /// Row `i`'s (`topo`, `defer_until`, `due`).
    pub fn cold(&self, i: usize) -> [u32; 3] {
        [
            u32_at(self.topo, i),
            u32_at(self.defer, i),
            u32_at(self.due, i),
        ]
    }

    fn list(c: &[&[u8]; 3], i: usize) -> Vec<(u8, u32)> {
        (u32_at(c[0], i) as usize..u32_at(c[0], i + 1) as usize)
            .map(|e| (c[2][e], u32_at(c[1], e)))
            .collect()
    }

    /// Row `i`'s out-list, `(kind, dst)`.
    pub fn out_list(&self, i: usize) -> Vec<(u8, u32)> {
        Columns::list(&self.out, i)
    }

    /// Row `i`'s in-list, `(kind, src)`.
    pub fn in_list(&self, i: usize) -> Vec<(u8, u32)> {
        Columns::list(&self.inn, i)
    }

    /// `IDS[i]`.
    pub fn id(&self, i: usize) -> u32 {
        u32_at(self.ids, i)
    }

    /// The bytes of bitset `i` of `BMDIR`.
    pub fn bitset(&self, i: usize) -> Result<&'a [u8], String> {
        self.view.need(tag::bm(i as u16))
    }

    /// The frozen bitset of `key` in a base.
    pub fn frozen(&self, key: BitsetKey) -> Result<Frozen<'a>, String> {
        let i = self
            .keys
            .iter()
            .position(|k| *k == key)
            .ok_or_else(|| format!("no bitset {key:?}"))?;
        Frozen::parse(self.bitset(i)?)
    }
}

/// The hot index of a segment: `NODE`, the six CSR columns, `BMDIR` and every `BM.<i>` ([AR §8.1]).
pub fn hot_bytes(v: &SegView<'_>) -> u64 {
    v.table
        .iter()
        .filter(|e| {
            e.tag == tag::NODE
                || (tag::OUT_OFF..=tag::IN_KIND).contains(&e.tag)
                || e.tag == tag::BMDIR
                || e.tag >= tag::bm(0)
        })
        .map(|e| e.len)
        .sum()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::seg::bitset::tests::frozen_len;
    use crate::seg::data_off;
    use std::collections::BTreeSet;

    pub(crate) fn spec(n: u32) -> Spec {
        Spec {
            n,
            upto_lsn: 1 << 16,
            base_seq: 999,
        }
    }

    /// Each section's length and `count` as [F09] gives them ([F09 §4.1], [F09 §4.5], [F09 §7.1], [F09 §9.2]) at N =
    /// 1,000 and N = 70,000 (two chunks): `NODE` 60·N, `CREATOR` 6·N, `TOPO`/`DEFER`/`DUE` 4·N, `PRIO` N after `DUE`
    /// ([F09 §5.4]), the offsets 4(N + 1), `*_DST`/`*_SRC` 4e and `*_KIND` e with e = 3N, `BMDIR` 8 + 4k; each frozen
    /// bitset as §4.5 sizes it; the sections placed from `data_off` = 120 + 32 · 18.
    #[test]
    fn base_sections_have_f09_sizes() {
        for n in [1_000u32, 70_000] {
            let s = spec(n);
            let b = build(&s).unwrap();
            let v = SegView::parse(&b).unwrap();
            let (nn, e) = (u64::from(n), 3 * u64::from(n));
            let mut want: Vec<(u16, u32, u64)> = vec![
                (tag::NODE, n, 60 * nn),
                (tag::CREATOR, n, 6 * nn),
                (tag::TOPO, n, 4 * nn),
                (tag::DEFER, n, 4 * nn),
                (tag::DUE, n, 4 * nn),
                (0x0009, n, nn),
                (tag::OUT_OFF, n + 1, 4 * (nn + 1)),
                (tag::OUT_DST, 3 * n, 4 * e),
                (tag::OUT_KIND, 3 * n, e),
                (tag::IN_OFF, n + 1, 4 * (nn + 1)),
                (tag::IN_SRC, 3 * n, 4 * e),
                (tag::IN_KIND, 3 * n, e),
                (tag::BMDIR, 5, 8 + 4 * 5),
            ];
            for (i, (_, m)) in s.bitsets().iter().enumerate() {
                want.push((tag::bm(i as u16), m.len() as u32, frozen_len(m) as u64));
            }
            let got: Vec<(u16, u32, u64)> =
                v.table.iter().map(|e| (e.tag, e.count, e.len)).collect();
            assert_eq!(got, want, "N = {n}");
            assert_eq!(v.table.len(), sections(5, false));
            assert_eq!(v.table[0].off, data_off(18) as u64);
            assert!(v.table.iter().all(|e| e.off % 8 == 0));
            assert_eq!(v.header, s.header());
            assert_eq!(v.header.n_rows, n);
        }
    }

    /// The hot index is about 99 B per node at 1e5 ([AR §8.1]: 60 B header + 8 B CSR offsets + 30 B edges + ~1 B
    /// bitsets), and the cold columns 19 B (`CREATOR` 6 + 3 × 4 + `PRIO` 1).
    #[test]
    fn hot_index_is_about_99_bytes_per_node() {
        let n = 100_000u32;
        let b = build(&spec(n)).unwrap();
        let v = SegView::parse(&b).unwrap();
        let per = hot_bytes(&v) as f64 / f64::from(n);
        assert!((98.0..=100.0).contains(&per), "{per} B per node");
        let cold: u64 = [tag::CREATOR, tag::TOPO, tag::DEFER, tag::DUE, tag::PRIO]
            .iter()
            .map(|&t| v.entry(t).unwrap().len)
            .sum();
        assert_eq!(cold, 19 * u64::from(n));
        v.check_digests().unwrap();
    }

    /// The CSR equals the naive edge lists ([F09 §7.1]): row #N's out-list is the generator's 3 edges
    /// sorted by (kind, dst), and its in-list holds exactly the (kind, src) of every edge into #N, sorted (I-P3).
    #[test]
    fn csr_equals_naive_edges() {
        let s = spec(2_000);
        let b = build(&s).unwrap();
        let c = Columns::parse(&b).unwrap();
        let mut naive_in: Vec<BTreeSet<(u8, u32)>> = vec![BTreeSet::new(); 2_001];
        for id in 1..=s.n {
            let l = s.out_edges(id);
            assert_eq!(l.len(), OUT_EDGES);
            assert!(
                l.windows(2).all(|w| w[0] < w[1])
                    && l.iter().all(|&(k, d)| d != id && (1..=EKINDS).contains(&k))
            );
            assert_eq!(c.out_list(id as usize - 1), l, "#{id}");
            for (k, d) in l {
                naive_in[d as usize].insert((k, id));
            }
        }
        let mut total = 0;
        for id in 1..=s.n {
            let want: Vec<(u8, u32)> = naive_in[id as usize].iter().copied().collect();
            total += want.len();
            assert_eq!(c.in_list(id as usize - 1), want, "#{id}");
        }
        assert_eq!(total, 3 * 2_000);
    }

    /// Every row reads back as the generator made it, and every bitset's members are the naive set of its key.
    #[test]
    fn base_rows_read_back() {
        let s = spec(3_000);
        let b = build(&s).unwrap();
        let c = Columns::parse(&b).unwrap();
        c.view.check_digests().unwrap();
        assert_eq!(c.rows, 3_000);
        assert!(c.ids.is_empty());
        for id in 1..=s.n {
            let i = id as usize - 1;
            assert_eq!(c.node(i), s.row(id));
            assert_eq!(c.creator(i), s.creator(id));
            assert_eq!(c.cold(i), [s.topo(id), s.defer(id), s.due(id)]);
        }
        assert_eq!(c.node(4).status, 1, "#5 has status 5 mod 4");
        let keys: Vec<BitsetKey> = s.bitsets().iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            [
                [1, 1, 0, 0],
                [2, 1, 0, 0],
                [2, 1, 1, 0],
                [2, 1, 2, 0],
                [2, 1, 3, 0]
            ]
        );
        assert_eq!(c.keys, keys);
        for (key, members) in s.bitsets() {
            let naive: Vec<u32> = (1..=s.n)
                .filter(|&id| key[0] == 1 || u32::from(key[2]) == id % 4)
                .collect();
            assert_eq!(members, naive);
            let f = c.frozen(key).unwrap();
            assert_eq!(f.members(), naive);
            let i = c.keys.iter().position(|k| *k == key).unwrap();
            assert_eq!(
                c.view.entry(tag::bm(i as u16)).unwrap().count as usize,
                naive.len()
            );
        }
        assert!(build(&spec(MIN_N - 1)).is_err());
    }

    /// [`build_with`]'s base (R-86 ruling 6) at N = 3,000: `PRIO` byte #N − 1 is the function's priority of #N, here
    /// [`ready_priority`], whose values P0–P4 all occur ([F08 §4]); byte 3 of every `NODE` row is 0 and every other
    /// `NODE` field is [`Spec::row`]'s ([F08 §3.1]: byte 3 is `_reserved`, R-66); and [`build`] is `build_with` at the
    /// default 2, byte for byte.
    #[test]
    fn build_with_puts_each_rows_priority_in_prio_and_zero_in_byte_3() {
        let s = spec(3_000);
        let b = build_with(&s, ready_priority).unwrap();
        let v = SegView::parse(&b).unwrap();
        let prio = v.need(0x0009).unwrap();
        assert_eq!(prio.len(), 3_000);
        let node = v.need(tag::NODE).unwrap();
        let mut seen = BTreeSet::new();
        for id in 1..=s.n {
            let i = id as usize - 1;
            assert_eq!(prio[i], ready_priority(id), "#{id}: PRIO");
            seen.insert(prio[i]);
            let row = &node[NODE_HDR_LEN * i..NODE_HDR_LEN * (i + 1)];
            assert_eq!(row[3], 0, "#{id}: byte 3");
            assert_eq!(NodeHdr::decode(row), s.row(id), "#{id}: the other fields");
        }
        assert_eq!(seen, (0..=4).collect::<BTreeSet<u8>>());
        assert_eq!(build(&s).unwrap(), build_with(&s, |_| 2).unwrap());
    }

    /// `Columns` requires `PRIO` ([F09 §3.1]: R in a base and every upper segment; R-86 ruling 6): [`build`]'s base
    /// written again with every section but `PRIO` is refused, naming `0x0009`.
    #[test]
    fn columns_refuse_a_segment_without_prio() {
        let b = build(&spec(1_000)).unwrap();
        let v = SegView::parse(&b).unwrap();
        let keep: Vec<_> = v.table.iter().filter(|e| e.tag != 0x0009).collect();
        let mut w = Writer::new(keep.len(), b.len());
        for e in &keep {
            let sec = v.section(e.tag).unwrap();
            w.push(e.tag, e.count, |o| {
                o.extend_from_slice(sec);
                Ok(())
            })
            .unwrap();
        }
        let without = w.finish(&v.header).unwrap();
        let r = Columns::parse(&without).map(|c| c.rows);
        assert!(
            r.as_ref().is_err_and(|e| e.contains("0x0009")),
            "Columns parsed a segment without 0x0009: {r:?}"
        );
    }

    /// The full base (u21 cut §3.2, D-4) at N = 3,000: [`build`]'s sections with the same bytes but `NODE`, whose rows are
    /// [`Spec::row`]'s with `title_off` and `fields_off` set, and ten more in tag order (`UID`, `TITLE_BLOB`,
    /// `FIELDS_BLOB`, an empty `BLOBTAB`, `DOCLEN`, `FTSSTAT`, `FILES`, `REFS`, `ALLOC`, `TREES`); `tok_ver` 1. Each row's
    /// title is 60 bytes of words of 3–9 lowercase letters, its block the abstract (field 2) and 1–3 of the fields 3–8,
    /// each 24 bytes of text; `UID` holds every (uid, #N) by uid; `ALLOC` from #1 (uid, 0, 1); `DOCLEN` each row's
    /// token counts and `FTSSTAT` their totals; `REFS`, `TREES` and `FILES` the generators' 50 refs, 4 trees and 12 files;
    /// `SecEnt.flags` bit 0 on `DOCLEN` and `FTSSTAT` and 0 on every other entry ([F09 §3.3]).
    #[test]
    fn full_base_rows_carry_titles_fields_and_uids() {
        let s = spec(3_000);
        let full = build_full(&s).unwrap();
        let plain = build(&s).unwrap();
        let (v, p) = (
            SegView::parse(&full).unwrap(),
            SegView::parse(&plain).unwrap(),
        );
        v.check_digests().unwrap();
        assert_eq!(
            v.header,
            Header {
                tok_ver: 1,
                ..s.header()
            }
        );
        let mut want: Vec<u16> = p.table.iter().map(|e| e.tag).collect();
        want.extend([
            tag::UID,
            tag::TITLE_BLOB,
            tag::FIELDS_BLOB,
            tag::BLOBTAB,
            tag::DOCLEN,
            tag::FTSSTAT,
            tag::FILES,
            tag::REFS,
            tag::ALLOC,
            tag::TREES,
        ]);
        want.sort_unstable();
        assert_eq!(v.table.iter().map(|e| e.tag).collect::<Vec<_>>(), want);
        assert_eq!(v.table.len(), full_sections(5));
        for e in &v.table {
            let want = u16::from(e.tag == 0x0052 || e.tag == 0x0053);
            assert_eq!(e.flags, want, "{:#06x} flags {}", e.tag, e.flags);
        }
        for e in p.table.iter().filter(|e| e.tag != tag::NODE) {
            assert_eq!(v.section(e.tag), p.section(e.tag), "{:#06x}", e.tag);
        }
        let c = Columns::parse(&full).unwrap();
        let vocab = Vocab::new();
        let (titles, fields) = (
            v.need(tag::TITLE_BLOB).unwrap(),
            v.need(tag::FIELDS_BLOB).unwrap(),
        );
        let doclen = text::decode_doclen(v.need(tag::DOCLEN).unwrap()).unwrap();
        let mut stat = [FtsStat::default(); 3];
        let mut names = std::collections::BTreeSet::new();
        for id in 1..=s.n {
            let h = c.node(id as usize - 1);
            assert_eq!(
                NodeHdr {
                    title_off: NONE32,
                    fields_off: NONE32,
                    ..h
                },
                s.row(id),
                "#{id}"
            );
            let title = crate::tail::enc::Reader::new(&titles[h.title_off as usize..])
                .vstr()
                .unwrap();
            assert_eq!(title.as_bytes(), &vocab.title(id)[..], "#{id} title");
            assert_eq!(title.len(), TITLE_LEN);
            assert!(
                title
                    .split(' ')
                    .all(|w| w.len() <= 9 && w.bytes().all(|b| b.is_ascii_lowercase()))
            );
            let block = crate::tail::enc::Reader::new(&fields[h.fields_off as usize..])
                .vbytes()
                .unwrap();
            assert_eq!(block, &full_block(&vocab, id)[..], "#{id} fields");
            let f = vocab.fields(id);
            assert!((2..=4).contains(&f.len()) && f[0].0 == ABSTRACT_FIELD);
            assert!(
                f.windows(2).all(|w| w[0].0 < w[1].0)
                    && f[1..].iter().all(|x| (3..=8).contains(&x.0))
            );
            assert!(f.iter().all(|x| x.1.len() == FIELD_LEN));
            names.extend(f.iter().map(|x| x.0));
            let t = [
                tokens(title.as_bytes()).unwrap(),
                tokens(&f[0].1).unwrap(),
                0,
            ];
            assert_eq!(doclen[id as usize - 1], t, "#{id} DOCLEN");
            for k in 0..2 {
                stat[k].docs += u32::from(t[k] > 0);
                stat[k].total_len += u64::from(t[k]);
            }
        }
        assert_eq!(names, (2..=8).collect(), "every field 2–8 occurs");
        assert_eq!(
            text::decode_ftsstat(v.need(tag::FTSSTAT).unwrap()).unwrap(),
            stat
        );
        let uid = fixed_rows(v.need(tag::UID).unwrap(), 20).unwrap();
        let mut want_uid: Vec<Vec<u8>> = (1..=s.n)
            .map(|id| [&full_uid(id)[..], &id.to_le_bytes()].concat())
            .collect();
        want_uid.sort_unstable();
        assert_eq!(uid, want_uid.iter().map(Vec::as_slice).collect::<Vec<_>>());
        let (first, alloc) = rt::decode_alloc(v.need(tag::ALLOC).unwrap()).unwrap();
        assert_eq!(first, 1);
        assert!(
            alloc
                .iter()
                .enumerate()
                .all(|(i, r)| (r.uid, r.ref_id, r.create_seq) == (full_uid(i as u32 + 1), 0, 1))
        );
        assert_eq!(alloc.len(), 3_000);
        assert_eq!(
            rt::decode_refs(v.need(tag::REFS).unwrap()).unwrap(),
            (full_refs(&s), REFS)
        );
        assert_eq!(
            rt::decode_trees(v.need(tag::TREES).unwrap()).unwrap(),
            full_trees()
        );
        assert_eq!(
            rt::decode_files(v.need(tag::FILES).unwrap()).unwrap(),
            full_files(&s)
        );
        assert_eq!(full_files(&s).len(), 12);
        assert_eq!(v.need(tag::BLOBTAB).unwrap(), &[0, 0, 0, 0, 36, 0, 0, 0]);
        assert!(build_full(&spec(MIN_N - 1)).is_err());
    }

    /// A segment's section directory before `PRIO`, with each section's XXH3: (tag, count, len, XXH3).
    pub(crate) type Pre = [(u16, u32, u64, u64)];

    /// Ruling 8's derivation (R-86): `bytes`, written with `PRIO`, follow from the pre-`PRIO` segment whose literals
    /// were recorded on the parent of the `PRIO` commit (4036cbf): the directory is `pre`'s with `(0x0009, rows, rows)`
    /// inserted after `DUE` (P-1), every other count and length unchanged; every section but `NODE` and `PRIO` has its
    /// recorded XXH3; `NODE` the recorded XXH3 of the old `NODE` with byte 3 of every row zeroed (`node0`); and
    /// `PRIO`'s bytes the recorded XXH3 of the old byte-3 sequence (`byte3`).
    pub(crate) fn follows_from_pre_prio(
        name: &str,
        bytes: &[u8],
        pre: &Pre,
        node0: u64,
        byte3: u64,
        rows: u32,
    ) {
        use xxhash_rust::xxh3::xxh3_64;
        let v = SegView::parse(bytes).unwrap();
        let prio = v.need(0x0009).unwrap();
        assert_eq!(
            (prio.len() as u64, v.entry(0x0009).unwrap().count),
            (u64::from(rows), rows),
            "{name}: PRIO's size"
        );
        assert_eq!(
            xxh3_64(prio),
            byte3,
            "{name}: PRIO is the old byte-3 sequence"
        );
        assert_eq!(
            xxh3_64(v.need(tag::NODE).unwrap()),
            node0,
            "{name}: NODE is the old NODE with byte 3 zeroed"
        );
        let mut want: Vec<(u16, u32, u64)> = Vec::new();
        for &(t, c, l, _) in pre {
            want.push((t, c, l));
            if t == tag::DUE {
                want.push((0x0009, rows, u64::from(rows)));
            }
        }
        let got: Vec<(u16, u32, u64)> = v.table.iter().map(|e| (e.tag, e.count, e.len)).collect();
        assert_eq!(got, want, "{name}: the directory");
        for &(t, _, _, x) in pre.iter().filter(|p| p.0 != tag::NODE) {
            assert_eq!(xxh3_64(v.section(t).unwrap()), x, "{name}: {t:#06x}");
        }
    }

    /// [`build`]'s base at N = 3,000 before `PRIO` (recorded on 4036cbf; annex E).
    const BUILD_PRE: [(u16, u32, u64, u64); 17] = [
        (0x0002, 3_000, 180_000, 0x5274_81af_a561_f807),
        (0x0003, 3_000, 18_000, 0xbf09_e4c2_2bb4_a185),
        (0x0004, 3_000, 12_000, 0x2e5a_4097_38e0_e148),
        (0x0005, 3_000, 12_000, 0xfe5a_4cf1_2b4c_e3db),
        (0x0006, 3_000, 12_000, 0xce85_b37e_1279_e668),
        (0x0020, 3_001, 12_004, 0xc5f0_c205_5737_9c58),
        (0x0021, 9_000, 36_000, 0x4ab1_de69_85bd_555f),
        (0x0022, 9_000, 9_000, 0xd3d4_c283_587f_4178),
        (0x0023, 3_001, 12_004, 0xc104_8752_0860_595c),
        (0x0024, 9_000, 36_000, 0xcdc3_f1d1_1657_1596),
        (0x0025, 9_000, 9_000, 0xcdca_64b9_de65_f78b),
        (0x0040, 5, 28, 0x1bbc_e98e_3e2d_3f11),
        (0x8000, 3_000, 6_024, 0xda80_6f78_1a9f_2711),
        (0x8001, 750, 1_524, 0x5f04_7e7e_f21a_9e4f),
        (0x8002, 750, 1_524, 0xa46b_d01b_ba47_3efd),
        (0x8003, 750, 1_524, 0x2a39_e399_7fde_22f6),
        (0x8004, 750, 1_524, 0xa965_a41c_d4fa_9961),
    ];

    /// [`build_with`]'s base at the [`ready_priority`] priorities, N = 3,000, before `PRIO` (recorded on 4036cbf).
    const BUILD_WITH_PRE: [(u16, u32, u64, u64); 17] = [
        (0x0002, 3_000, 180_000, 0x748f_e4af_cd9a_dc86),
        (0x0003, 3_000, 18_000, 0xbf09_e4c2_2bb4_a185),
        (0x0004, 3_000, 12_000, 0x2e5a_4097_38e0_e148),
        (0x0005, 3_000, 12_000, 0xfe5a_4cf1_2b4c_e3db),
        (0x0006, 3_000, 12_000, 0xce85_b37e_1279_e668),
        (0x0020, 3_001, 12_004, 0xc5f0_c205_5737_9c58),
        (0x0021, 9_000, 36_000, 0x4ab1_de69_85bd_555f),
        (0x0022, 9_000, 9_000, 0xd3d4_c283_587f_4178),
        (0x0023, 3_001, 12_004, 0xc104_8752_0860_595c),
        (0x0024, 9_000, 36_000, 0xcdc3_f1d1_1657_1596),
        (0x0025, 9_000, 9_000, 0xcdca_64b9_de65_f78b),
        (0x0040, 5, 28, 0x1bbc_e98e_3e2d_3f11),
        (0x8000, 3_000, 6_024, 0xda80_6f78_1a9f_2711),
        (0x8001, 750, 1_524, 0x5f04_7e7e_f21a_9e4f),
        (0x8002, 750, 1_524, 0xa46b_d01b_ba47_3efd),
        (0x8003, 750, 1_524, 0x2a39_e399_7fde_22f6),
        (0x8004, 750, 1_524, 0xa965_a41c_d4fa_9961),
    ];

    /// Ruling 8's derivation of the two plain-base pins: [`build`]'s and [`build_with`]'s bytes at N = 3,000 follow
    /// from their pre-`PRIO` bytes ([`follows_from_pre_prio`]); the two old `NODE`s differ only in byte 3, so their
    /// zeroed hashes are equal.
    #[test]
    fn plain_base_bytes_follow_from_the_pre_prio_bytes() {
        let s = spec(3_000);
        follows_from_pre_prio(
            "build",
            &build(&s).unwrap(),
            &BUILD_PRE,
            0x0518_4c8e_5ad2_460a,
            0x4a1c_5a68_7665_c786,
            3_000,
        );
        follows_from_pre_prio(
            "build_with(ready_priority)",
            &build_with(&s, ready_priority).unwrap(),
            &BUILD_WITH_PRE,
            0x0518_4c8e_5ad2_460a,
            0x73e2_0345_adad_9234,
            3_000,
        );
    }

    /// The plain base keeps its bytes (u21 cut §6, the identity gate's quick form): the XXH3-64 of [`build`] and of
    /// [`build_with`] at the [`ready_priority`] priorities, N = 3,000. Pinned at 8916d63, before the full base's change
    /// of the writers' helpers; re-pinned when `PRIO` moved `priority` out of byte 3 (R-66), with the new bytes derived
    /// from the old ones by `plain_base_bytes_follow_from_the_pre_prio_bytes` (R-86 ruling 8).
    #[test]
    fn plain_base_bytes_are_unchanged() {
        let s = spec(3_000);
        assert_eq!(
            xxhash_rust::xxh3::xxh3_64(&build(&s).unwrap()),
            0x50d0_1c05_37e6_0eb1,
            "build"
        );
        assert_eq!(
            xxhash_rust::xxh3::xxh3_64(&build_with(&s, ready_priority).unwrap()),
            0xbcbf_336b_0ff2_3cb6,
            "build_with(ready_priority)"
        );
    }
}
