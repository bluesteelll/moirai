//! The full-text sections of a segment at `tok_ver` 1 ([F09 §12]): tokenizer v1 restricted
//! to ASCII, the term index of a set of documents, `TERMS` (front-coded, blocks of 16), `POST`, `DOCLEN` and
//! `FTSSTAT`, and their readers for the tests.
//!
//! - **Tokenizer v1, ASCII only** ([F09 §12.1]; u20 cut F-5). Tokenizer v1 needs the UCD 17.0.0 tables
//!   ([F09 §12.1]), which the probe does not have. On ASCII text it is exact: a token is a maximal run of `[A-Za-z0-9]`
//!   (letters and decimal digits), each byte mapped to lowercase, the term cut to 64 bytes; every other ASCII byte
//!   separates tokens. A byte above `0x7F` is refused with an error that names F-5. A field's length is its token count.
//! - **Documents** ([F09 §12.3]): a non-absent, non-deleted row; its three fields are its title, its abstract
//!   ([F08 §9]'s field 2 by the probe's convention) and its body. The caller passes each field's text; the probe's
//!   bodies are random bytes (`tail/synth.rs`), so its caller passes them as empty (F-5).
//! - **`TERMS`** ([F09 §12.2]): `n_terms`, `n_blocks` = ⌈n_terms / 16⌉, `block_off[n_blocks]` from the section's
//!   first byte, then the blocks; the first term of a block is `len`, its bytes and `post_off` (`uvar64`); every later
//!   term `shared` (maximal), `suffix_len`, the suffix and `post_off`. `count` = `n_terms`.
//! - **`POST`** ([F09 §12.3]): per term in term order, `n_docs` (`uvar32`), then per posting in ascending `#N`
//!   `ddoc` (`uvar32`, the first `#N`, then the difference), `fmask` (bit 0 title, 1 abstract, 2 body) and one `tf`
//!   (`uvar16`) per set bit in bit order. `count` = `n_terms`.
//! - **`DOCLEN`** ([F09 §12.4]): `[3]u16` per row, each count saturating at 65,535; zero for absent and deleted
//!   rows. **`FTSSTAT`** ([F09 §12.4]): 48 bytes, three records (title, abstract, body) of `docs` `u32`, `_reserved`
//!   `u32` and `total_len` `u64`; `count` = 3. Both are `HOLE(F09-doclen)`'s "kept" candidate ([F09 Holes]), which
//!   measurement 14 writes as an upper bound of that hole's cost (u20 cut F-6).

use super::{u32_at, u64_at};
use crate::tail::enc::{Reader, put_uvar};
use std::collections::BTreeMap;

/// The segment's `tok_ver` when it carries these sections ([F09 §2.3], [F09 §12]).
pub const TOK_VER: u8 = 1;
/// The longest term, in bytes ([F09 §12.1]).
pub const TERM_MAX: usize = 64;
/// Terms per `TERMS` block ([F09 §12.2]).
pub const BLOCK: usize = 16;
/// The fields of a document, in `fmask` bit order: title, abstract, body ([F09 §12.3]).
pub const FIELDS: usize = 3;
/// `FTSSTAT`'s length ([F09 §12.4]).
pub const FTSSTAT_LEN: usize = 48;

/// Calls `each` with every term of `text` in order (tokenizer v1 on ASCII, §12.1) and returns the token count. A byte
/// above `0x7F` is refused: tokenizer v1 classifies scalar values by the UCD 17.0.0 tables, which the probe does not
/// carry (u20 cut F-5).
pub fn tokenize(text: &[u8], mut each: impl FnMut(&[u8])) -> Result<u32, String> {
    let mut term = [0u8; TERM_MAX];
    let (mut len, mut run, mut count) = (0usize, false, 0u32);
    for (i, &b) in text.iter().enumerate() {
        if b > 0x7F {
            return Err(format!(
                "byte {b:#04x} at {i}: tokenizer v1 needs the UCD 17.0.0 tables, outside the probe (u20 cut F-5)"
            ));
        }
        if b.is_ascii_alphanumeric() {
            if len < TERM_MAX {
                term[len] = b.to_ascii_lowercase();
                len += 1;
            }
            run = true;
        } else if run {
            each(&term[..len]);
            count += 1;
            (len, run) = (0, false);
        }
    }
    if run {
        each(&term[..len]);
        count += 1;
    }
    Ok(count)
}

/// One document of a segment's index: its `#N` and the text of its title, abstract and body (each possibly empty).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Doc<'a> {
    /// `#N`.
    pub id: u32,
    /// Title, abstract, body.
    pub fields: [&'a [u8]; FIELDS],
}

/// A document's `DOCLEN` entry: the token count of each field, saturating at 65,535 ([F09 §12.4]).
pub fn doclen(doc: &Doc<'_>) -> Result<[u16; FIELDS], String> {
    let mut out = [0u16; FIELDS];
    for (f, text) in doc.fields.iter().enumerate() {
        let n = tokenize(text, |_| {})?;
        out[f] = u16::try_from(n).unwrap_or(u16::MAX);
    }
    Ok(out)
}

/// One term's postings: `(#N, tf per field)` in ascending `#N`.
pub type Postings = Vec<(u32, [u16; FIELDS])>;

/// The term index of a segment's documents: each term's postings, `(#N, tf per field)` in ascending `#N`, the terms
/// in ascending bytewise order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextIndex {
    /// Term → postings.
    pub terms: BTreeMap<Vec<u8>, Postings>,
}

impl TextIndex {
    /// The index of `docs`, which must be in strictly ascending `#N`. A term frequency above 65,535 (`tf` is a
    /// `uvar16`, [F09 §12.3]) is refused.
    pub fn of(docs: &[Doc<'_>]) -> Result<TextIndex, String> {
        let mut ix = TextIndex::default();
        let mut counts: BTreeMap<Vec<u8>, [u32; FIELDS]> = BTreeMap::new();
        for (k, d) in docs.iter().enumerate() {
            if k > 0 && docs[k - 1].id >= d.id {
                return Err(format!(
                    "documents #{} then #{}: not ascending",
                    docs[k - 1].id,
                    d.id
                ));
            }
            counts.clear();
            for (f, text) in d.fields.iter().enumerate() {
                tokenize(text, |t| match counts.get_mut(t) {
                    Some(c) => c[f] += 1,
                    None => {
                        let mut c = [0u32; FIELDS];
                        c[f] = 1;
                        counts.insert(t.to_vec(), c);
                    }
                })?;
            }
            for (t, c) in &counts {
                let mut tf = [0u16; FIELDS];
                for f in 0..FIELDS {
                    tf[f] = u16::try_from(c[f])
                        .map_err(|_| format!("#{}: a term frequency of {} (uvar16)", d.id, c[f]))?;
                }
                match ix.terms.get_mut(t) {
                    Some(p) => p.push((d.id, tf)),
                    None => {
                        ix.terms.insert(t.clone(), vec![(d.id, tf)]);
                    }
                }
            }
        }
        Ok(ix)
    }

    /// The number of distinct terms, `TERMS`' and `POST`'s `count`.
    pub fn n_terms(&self) -> u32 {
        self.terms.len() as u32
    }
}

/// Appends `POST` (§12.3) and returns each term's offset from its first byte, in term order.
pub fn encode_post(ix: &TextIndex, out: &mut Vec<u8>) -> Result<Vec<u64>, String> {
    let base = out.len();
    let mut offs = Vec::with_capacity(ix.terms.len());
    for (term, postings) in &ix.terms {
        if postings.is_empty() {
            return Err(format!("term {term:?} has no posting"));
        }
        offs.push((out.len() - base) as u64);
        put_uvar(out, postings.len() as u64);
        let mut prev = 0u32;
        for (k, &(id, tf)) in postings.iter().enumerate() {
            if k > 0 && id <= prev {
                return Err(format!("term {term:?}: postings #{prev} then #{id}"));
            }
            put_uvar(out, u64::from(id - prev));
            prev = id;
            let fmask = (0..FIELDS).fold(0u8, |m, f| m | (u8::from(tf[f] > 0) << f));
            if fmask == 0 {
                return Err(format!("term {term:?} #{id}: a posting with no field"));
            }
            out.push(fmask);
            for &t in tf.iter().filter(|&&t| t > 0) {
                put_uvar(out, u64::from(t));
            }
        }
    }
    Ok(offs)
}

/// Appends `TERMS` (§12.2) with each term's `post_off` from `post_off`, in term order.
pub fn encode_terms(ix: &TextIndex, post_off: &[u64], out: &mut Vec<u8>) -> Result<(), String> {
    if post_off.len() != ix.terms.len() {
        return Err(format!(
            "{} post offsets for {} terms",
            post_off.len(),
            ix.terms.len()
        ));
    }
    let base = out.len();
    let n = ix.terms.len();
    let blocks = n.div_ceil(BLOCK);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&(blocks as u32).to_le_bytes());
    let dir = out.len();
    out.resize(dir + 4 * blocks, 0);
    let mut prev: &[u8] = &[];
    for (k, (term, &po)) in ix.terms.keys().zip(post_off).enumerate() {
        if term.is_empty() || term.len() > TERM_MAX {
            return Err(format!("a term of {} bytes", term.len()));
        }
        if k % BLOCK == 0 {
            let at = (out.len() - base) as u32;
            let slot = dir + 4 * (k / BLOCK);
            out[slot..slot + 4].copy_from_slice(&at.to_le_bytes());
            out.push(term.len() as u8);
            out.extend_from_slice(term);
        } else {
            let shared = prev
                .iter()
                .zip(term.iter())
                .take_while(|(a, b)| a == b)
                .count();
            out.push(shared as u8);
            out.push((term.len() - shared) as u8);
            out.extend_from_slice(&term[shared..]);
        }
        put_uvar(out, po);
        prev = term;
    }
    Ok(())
}

/// Reads `TERMS` back: every term with its `post_off`, in order; refuses a block offset that does not land on its
/// block, a `shared` that is not maximal, or terms out of order.
pub fn decode_terms(b: &[u8]) -> Result<Vec<(Vec<u8>, u64)>, String> {
    if b.len() < 8 {
        return Err("TERMS shorter than its header".into());
    }
    let (n, blocks) = (u32_at(b, 0) as usize, u32_at(b, 1) as usize);
    if blocks != n.div_ceil(BLOCK) || b.len() < 8 + 4 * blocks {
        return Err(format!("TERMS: {n} terms in {blocks} blocks"));
    }
    let mut r = Reader::new(b);
    r.take(8 + 4 * blocks).map_err(|e| e.to_string())?;
    let mut out: Vec<(Vec<u8>, u64)> = Vec::with_capacity(n);
    for k in 0..n {
        let term = if k % BLOCK == 0 {
            if u32_at(b, 2 + k / BLOCK) as usize != r.pos() {
                return Err(format!(
                    "block {}'s offset is not where it starts",
                    k / BLOCK
                ));
            }
            let len = usize::from(r.u8().map_err(|e| e.to_string())?);
            r.take(len).map_err(|e| e.to_string())?.to_vec()
        } else {
            let shared = usize::from(r.u8().map_err(|e| e.to_string())?);
            let suffix = usize::from(r.u8().map_err(|e| e.to_string())?);
            let prev = &out[k - 1].0;
            if shared > prev.len() || suffix == 0 {
                return Err(format!("term {k}: shared {shared}, suffix {suffix}"));
            }
            let mut t = prev[..shared].to_vec();
            t.extend_from_slice(r.take(suffix).map_err(|e| e.to_string())?);
            if prev.get(shared).is_some_and(|&c| c == t[shared]) {
                return Err(format!("term {k}: shared {shared} is not maximal"));
            }
            t
        };
        if term.is_empty() || term.len() > TERM_MAX || out.last().is_some_and(|(p, _)| *p >= term) {
            return Err(format!("term {k} is empty, too long or out of order"));
        }
        let po = r.uvar64().map_err(|e| e.to_string())?;
        out.push((term, po));
    }
    r.end().map_err(|e| e.to_string())?;
    Ok(out)
}

/// Reads the posting list at `off` of `POST`: `(#N, tf per field)` in ascending `#N`, and the offset where it ends.
pub fn decode_post(b: &[u8], off: u64) -> Result<(Postings, u64), String> {
    let rest = b
        .get(off as usize..)
        .ok_or_else(|| format!("post_off {off} past POST's {} bytes", b.len()))?;
    let mut r = Reader::new(rest);
    let e = |e: crate::tail::enc::DecodeError| e.to_string();
    let n = r.uvar32().map_err(e)?;
    if n == 0 {
        return Err(format!("a posting list of no document at {off}"));
    }
    let mut out = Vec::with_capacity(n as usize);
    let mut id = 0u32;
    for k in 0..n {
        let d = r.uvar32().map_err(e)?;
        if k > 0 && d == 0 {
            return Err(format!("a ddoc of 0 at {off}"));
        }
        id = id.checked_add(d).ok_or("a #N past 2^32 − 1")?;
        let fmask = r.u8().map_err(e)?;
        if fmask == 0 || fmask & 0xF8 != 0 {
            return Err(format!("#{id}: fmask {fmask:#04x}"));
        }
        let mut tf = [0u16; FIELDS];
        for (f, t) in tf.iter_mut().enumerate() {
            if fmask & (1 << f) != 0 {
                *t = r.uvar16().map_err(e)?;
                if *t == 0 {
                    return Err(format!("#{id}: a tf of 0"));
                }
            }
        }
        out.push((id, tf));
    }
    Ok((out, off + r.pos() as u64))
}

/// Reads `TERMS` and `POST` back into the index; refuses posting lists that are not contiguous in term order from
/// `POST`'s first byte to its last.
pub fn decode_index(terms: &[u8], post: &[u8]) -> Result<TextIndex, String> {
    let mut ix = TextIndex::default();
    let mut end = 0u64;
    for (term, off) in decode_terms(terms)? {
        if off != end {
            return Err(format!(
                "term {term:?}'s posting list at {off}, the previous ends at {end}"
            ));
        }
        let (p, next) = decode_post(post, off)?;
        end = next;
        ix.terms.insert(term, p);
    }
    if end != post.len() as u64 {
        return Err(format!("POST holds {} bytes, its lists {end}", post.len()));
    }
    Ok(ix)
}

/// Appends `DOCLEN`: one `[3]u16` per row.
pub fn put_doclen(out: &mut Vec<u8>, rows: &[[u16; FIELDS]]) {
    out.reserve(6 * rows.len());
    for r in rows {
        for c in r {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }
}

/// Reads `DOCLEN` back.
pub fn decode_doclen(b: &[u8]) -> Result<Vec<[u16; FIELDS]>, String> {
    if !b.len().is_multiple_of(6) {
        return Err(format!("DOCLEN of {} bytes", b.len()));
    }
    Ok(b.as_chunks::<6>()
        .0
        .iter()
        .map(|c| {
            [
                u16::from_le_bytes([c[0], c[1]]),
                u16::from_le_bytes([c[2], c[3]]),
                u16::from_le_bytes([c[4], c[5]]),
            ]
        })
        .collect())
}

/// One `FTSSTAT` record: the view's documents whose field has a token, and the field's total token count.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FtsStat {
    /// `docs`.
    pub docs: u32,
    /// `total_len`.
    pub total_len: u64,
}

/// Appends `FTSSTAT`: title, abstract, body.
pub fn put_ftsstat(out: &mut Vec<u8>, s: &[FtsStat; FIELDS]) {
    for r in s {
        out.extend_from_slice(&r.docs.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&r.total_len.to_le_bytes());
    }
}

/// Reads `FTSSTAT` back; refuses another length or a non-zero `_reserved`.
pub fn decode_ftsstat(b: &[u8]) -> Result<[FtsStat; FIELDS], String> {
    if b.len() != FTSSTAT_LEN {
        return Err(format!("FTSSTAT of {} bytes, not 48", b.len()));
    }
    let mut s = [FtsStat::default(); FIELDS];
    for (f, r) in s.iter_mut().enumerate() {
        let at = 16 * f;
        if u32_at(b, at / 4 + 1) != 0 {
            return Err(format!("FTSSTAT record {f}'s _reserved is not zero"));
        }
        *r = FtsStat {
            docs: u32_at(b, at / 4),
            total_len: u64_at(b, at + 8),
        };
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms_of(text: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut v = Vec::new();
        tokenize(text, |t| v.push(t.to_vec()))?;
        Ok(v)
    }

    fn s(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|x| x.as_bytes().to_vec()).collect()
    }

    /// Tokenizer v1 on ASCII ([F09 §12.1]): maximal runs of letters and digits, mapped to
    /// lowercase; every other byte, `_` and `-` included, separates; a run longer than 64 bytes is cut to 64; the count
    /// is the number of tokens; a byte above `0x7F` is refused naming F-5.
    #[test]
    fn tokenizer_v1_on_ascii_text() {
        assert_eq!(
            terms_of(b"Hello, World! foo_bar 42x-Y").unwrap(),
            s(&["hello", "world", "foo", "bar", "42x", "y"])
        );
        assert_eq!(tokenize(b"Hello, World! foo_bar 42x-Y", |_| {}), Ok(6));
        assert_eq!(terms_of(b"  ,; ").unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(tokenize(b"", |_| {}), Ok(0));
        assert_eq!(terms_of(b"abc abc").unwrap(), s(&["abc", "abc"]), "tf 2");
        let long = "A".repeat(70) + " b";
        assert_eq!(
            terms_of(long.as_bytes()).unwrap(),
            vec![vec![b'a'; 64], b"b".to_vec()],
            "cut to 64 bytes, still one token"
        );
        let err = tokenize("caf\u{e9}".as_bytes(), |_| {}).unwrap_err();
        assert!(err.contains("F-5") && err.contains("0xc3"), "{err}");
        assert_eq!(
            doclen(&Doc {
                id: 1,
                fields: [b"a b c", b"", b"x"],
            }),
            Ok([3, 0, 1])
        );
    }

    /// The term-dictionary test's title of #2: 18 distinct terms, in ascending order "ab", "abc", "abd", "b", then
    /// "c00" … "c13".
    fn eighteen_title() -> String {
        let mut title = String::from("abd ab abc B ");
        for i in 0..14 {
            title.push_str(&format!("c{i:02} "));
        }
        title
    }

    /// The index of #2 with that title and #9 with the abstract "ab ab", written out without the tokenizer: every term
    /// in #2's title once, and "ab" in #9's abstract twice.
    fn eighteen() -> TextIndex {
        let mut terms: BTreeMap<Vec<u8>, Postings> = BTreeMap::new();
        let mut names: Vec<String> = ["ab", "abc", "abd", "b"].map(String::from).to_vec();
        names.extend((0..14).map(|i| format!("c{i:02}")));
        for t in names {
            terms.insert(t.into_bytes(), vec![(2, [1, 0, 0])]);
        }
        terms.get_mut(&b"ab"[..]).unwrap().push((9, [0, 2, 0]));
        TextIndex { terms }
    }

    /// `TERMS` ([F09 §12.2]): `n_terms` 18, `n_blocks` 2, `block_off` from the section's
    /// first byte; block 0's first term as `len`, bytes, `post_off`; a later term as `shared` (maximal), `suffix_len`,
    /// the suffix, `post_off`; block 1 (term 16) again a whole term; `post_off` the offsets `POST` gives. It reads back,
    /// a `shared` that is not maximal is refused, and the tokenizer's index of the two documents is this one.
    #[test]
    fn terms_are_front_coded_in_blocks_of_16() {
        let ix = eighteen();
        assert_eq!(ix.n_terms(), 18);
        let mut post = Vec::new();
        let offs = encode_post(&ix, &mut post).unwrap();
        let mut b = Vec::new();
        encode_terms(&ix, &offs, &mut b).unwrap();
        assert_eq!(
            b.get(0..8),
            Some(&[18, 0, 0, 0, 2, 0, 0, 0][..]),
            "n_terms, n_blocks; TERMS is {} bytes",
            b.len()
        );
        assert_eq!(
            &b[8..12],
            &16u32.to_le_bytes(),
            "block 0 after the directory"
        );
        // Block 0: "ab" (2, 'a', 'b', post_off 0), then "abc" (shared 2, suffix 1, 'c', its post_off).
        assert_eq!(&b[16..20], &[2, b'a', b'b', 0]);
        assert_eq!(offs[0], 0);
        assert_eq!(&b[20..24], &[2, 1, b'c', offs[1] as u8]);
        // "abd": shared 2 with "abc", suffix "d"; "b": shared 0, suffix "b".
        assert_eq!(&b[24..28], &[2, 1, b'd', offs[2] as u8]);
        assert_eq!(&b[28..32], &[0, 1, b'b', offs[3] as u8]);
        // "c00": shared 0, suffix "c00"; "c01": shared 2, suffix "1".
        assert_eq!(&b[32..38], &[0, 3, b'c', b'0', b'0', offs[4] as u8]);
        assert_eq!(&b[38..42], &[2, 1, b'1', offs[5] as u8]);
        let b1 = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        assert_eq!(
            &b[b1..b1 + 5],
            &[3, b'c', b'1', b'2', offs[16] as u8],
            "block 1 begins with the whole term c12"
        );
        let back = decode_terms(&b).unwrap();
        let want: Vec<(Vec<u8>, u64)> =
            ix.terms.keys().cloned().zip(offs.iter().copied()).collect();
        assert_eq!(back, want);
        assert_eq!(decode_index(&b, &post).unwrap(), ix);
        // "abc" written with a shared prefix of 1 and the suffix "bc" (block 1's offset moved by the byte) is refused:
        // `shared` is maximal.
        let mut bad = [&b[..20], &[1, 2, b'b', b'c', offs[1] as u8], &b[24..]].concat();
        bad[12..16].copy_from_slice(&(b1 as u32 + 1).to_le_bytes());
        assert!(
            decode_terms(&bad).unwrap_err().contains("not maximal"),
            "{:?}",
            decode_terms(&bad)
        );
        let title = eighteen_title();
        let of = TextIndex::of(&[
            Doc {
                id: 2,
                fields: [title.as_bytes(), b"", b""],
            },
            Doc {
                id: 9,
                fields: [b"", b"ab ab", b""],
            },
        ])
        .unwrap();
        assert_eq!(of, ix, "the tokenizer's index");
    }

    /// `POST` ([F09 §12.3]): per term `n_docs`, then per posting `ddoc` (the first `#N`, then
    /// the difference), `fmask` (title 1, abstract 2, body 4) and one `tf` per set bit in bit order: literal bytes of an
    /// index written out, which is the tokenizer's index of its three documents. Documents come in ascending `#N`.
    #[test]
    fn postings_follow_f09_12_3() {
        let ix = TextIndex {
            terms: [
                (b"cat".to_vec(), vec![(5, [2, 0, 0]), (7, [0, 1, 2])]),
                (b"dog".to_vec(), vec![(5, [1, 1, 0]), (300, [1, 2, 0])]),
            ]
            .into(),
        };
        let mut post = Vec::new();
        let offs = encode_post(&ix, &mut post).unwrap();
        let cat = [2, 5, 0b001, 2, 2, 0b110, 1, 2];
        let dog = [2, 5, 0b011, 1, 1, 0xA7, 0x02, 0b011, 1, 2];
        assert_eq!(post, [&cat[..], &dog[..]].concat(), "POST's literal bytes");
        assert_eq!(offs, [0, cat.len() as u64]);
        assert_eq!(
            decode_post(&post, offs[1]).unwrap(),
            (vec![(5, [1, 1, 0]), (300, [1, 2, 0])], post.len() as u64)
        );
        let of = TextIndex::of(&[
            Doc {
                id: 5,
                fields: [b"cat dog cat", b"dog", b""],
            },
            Doc {
                id: 7,
                fields: [b"", b"cat", b"cat cat"],
            },
            Doc {
                id: 300,
                fields: [b"dog", b"dog dog", b""],
            },
        ])
        .unwrap();
        assert_eq!(of, ix, "the tokenizer's index");
        assert!(
            TextIndex::of(&[
                Doc {
                    id: 3,
                    fields: [b"a", b"", b""]
                },
                Doc {
                    id: 3,
                    fields: [b"b", b"", b""]
                }
            ])
            .is_err(),
            "documents in ascending #N"
        );
    }

    /// `DOCLEN` and `FTSSTAT` ([F09 §12.4]): `FTSSTAT` is exactly 48 bytes, three records of
    /// `docs` `u32`, `_reserved` 0 and `total_len` `u64`, in title, abstract, body order; `DOCLEN` is `[3]u16` per row,
    /// little-endian, a count saturating at 65,535. Both read back.
    #[test]
    fn doclen_and_ftsstat_follow_f09_12_4() {
        let stat = [
            FtsStat {
                docs: 0x0403_0201,
                total_len: 0x0C0B_0A09_0807_0605,
            },
            FtsStat {
                docs: 7,
                total_len: 8,
            },
            FtsStat::default(),
        ];
        let mut f = Vec::new();
        put_ftsstat(&mut f, &stat);
        assert_eq!(f.len(), 48, "48 B wanted, {} written", f.len());
        assert_eq!(
            &f[0..16],
            &[1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 9, 10, 11, 12]
        );
        assert_eq!(&f[16..20], &[7, 0, 0, 0]);
        assert_eq!(&f[24..32], &8u64.to_le_bytes());
        assert_eq!(&f[32..48], &[0; 16]);
        assert_eq!(decode_ftsstat(&f).unwrap(), stat);
        let mut bad = f.clone();
        bad[4] = 1;
        assert!(decode_ftsstat(&bad).is_err());
        let rows = [[2, 65_535, 0], [0, 0, 0], [0x0102, 3, 0x0405]];
        let mut b = Vec::new();
        put_doclen(&mut b, &rows);
        assert_eq!(
            b,
            [2, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 3, 0, 5, 4],
            "DOCLEN's literal bytes"
        );
        assert_eq!(decode_doclen(&b).unwrap(), rows);
        let many = "w ".repeat(70_000);
        let d = doclen(&Doc {
            id: 1,
            fields: [b"one two", many.as_bytes(), b""],
        })
        .unwrap();
        assert_eq!(d, rows[0], "saturates at 65,535");
    }
}
