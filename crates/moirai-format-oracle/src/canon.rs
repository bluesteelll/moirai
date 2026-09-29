//! [F07] the canonical items the oracle re-derives: the commit input C and `commit_id` (§3), the commit-kind names (§4),
//! message normalisation N and N_imp (§5), the typed value `cv` (§7.1) and the `changeset_digest` framing over item-10
//! entries (§10.4). Item 10's entries from states are the reference model's (WP-91); the oracle takes them, or the
//! stored digest, as given.

use crate::prim::{Oid, Reader, Result as DResult, blake3_256, err, lp};
use crate::value::check_f64;

/// The ten canonical items of one commit ([F07 §3.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Items {
    /// Item 1: the kind's canonical name.
    pub kind: String,
    /// Item 2: the parents' stated ids.
    pub parents: Vec<[u8; 32]>,
    /// Item 3.
    pub hlc: u64,
    /// Item 4.
    pub actor: String,
    /// Item 4.
    pub role: String,
    /// Item 4.
    pub session: String,
    /// Item 5: `git_algo` name, or empty.
    pub git_algo: String,
    /// Item 5: `git_head` digest bytes, or empty.
    pub git_head: Vec<u8>,
    /// Item 5.
    pub git_branch: String,
    /// Item 5.
    pub git_worktree: String,
    /// Item 5: `git_base` digest bytes, or empty.
    pub git_base: Vec<u8>,
    /// Item 6: the normalised message.
    pub message: String,
    /// Item 7.
    pub schema_version: u32,
    /// Item 8: the origin's full id.
    pub origin: Option<[u8; 32]>,
    /// Item 9: the foreign git object.
    pub foreign: Oid,
    /// Item 10 through its digest.
    pub changeset_digest: [u8; 32],
}

impl Items {
    /// The commit input C ([F07 §3.1]).
    pub fn input(&self) -> Vec<u8> {
        let mut c = Vec::with_capacity(256);
        lp(&mut c, b"moirai-commit-v1");
        lp(&mut c, self.kind.as_bytes());
        c.extend_from_slice(&(self.parents.len() as u32).to_le_bytes());
        for p in &self.parents {
            c.extend_from_slice(p);
        }
        c.extend_from_slice(&self.hlc.to_le_bytes());
        lp(&mut c, self.actor.as_bytes());
        lp(&mut c, self.role.as_bytes());
        lp(&mut c, self.session.as_bytes());
        lp(&mut c, self.git_algo.as_bytes());
        lp(&mut c, &self.git_head);
        lp(&mut c, self.git_branch.as_bytes());
        lp(&mut c, self.git_worktree.as_bytes());
        lp(&mut c, &self.git_base);
        lp(&mut c, self.message.as_bytes());
        c.extend_from_slice(&self.schema_version.to_le_bytes());
        lp(&mut c, self.origin.as_ref().map_or(&[][..], |o| &o[..]));
        let (fa, fo): (&[u8], &[u8]) = match &self.foreign {
            Oid::None => (b"", b""),
            Oid::Sha1(d) => (b"sha1", d),
            Oid::Sha256(d) => (b"sha256", d),
        };
        lp(&mut c, fa);
        lp(&mut c, fo);
        c.extend_from_slice(&self.changeset_digest);
        c
    }

    /// `commit_id = BLAKE3-256(C)` ([F07 §3.1]).
    pub fn commit_id(&self) -> [u8; 32] {
        blake3_256(&self.input())
    }
}

/// `changeset_digest` over item-10 entries already encoded and in §10.3 order ([F07 §10.4]):
/// `BLAKE3-256(lp("moirai-changeset-v1") ‖ entries ‖ u64 n)`.
pub fn changeset_digest(entries: &[Vec<u8>]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    let mut d = Vec::new();
    lp(&mut d, b"moirai-changeset-v1");
    h.update(&d);
    for e in entries {
        h.update(e);
    }
    h.update(&(entries.len() as u64).to_le_bytes());
    *h.finalize().as_bytes()
}

/// A canonical `path` payload ([F07 §7.1] tag 12): the root by name and the exact path text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CvPath {
    /// The root's name ([F07 §2.3]).
    pub root: String,
    /// The path text's exact bytes.
    pub text: String,
}

/// A canonical typed value `cv` ([F07 §7.1]): the canonical tag and its payload. The tags are [F07]'s own, never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cv {
    /// Tag 0.
    Absent,
    /// Tags 1 (`false`) and 2 (`true`).
    Bool(bool),
    /// Tag 3.
    Int(i64),
    /// Tag 4: a counter's total.
    Counter(i64),
    /// Tag 5: the bit pattern, never −0.0, NaN or ±∞.
    F64(u64),
    /// Tag 6: the value's name.
    Enum(String),
    /// Tag 7: non-empty UTF-8.
    Text(String),
    /// Tag 9: the element tag and the elements, in the bytewise order of their payloads ([F07 §2.4]).
    Set(u8, Vec<Cv>),
    /// Tag 10: a uid.
    Ref([u8; 16]),
    /// Tag 11: a full commit id.
    CommitRef([u8; 32]),
    /// Tag 12.
    Path(CvPath),
    /// Tag 13: `sha1` or `sha256`, never `none`.
    Oid(Oid),
    /// Tag 14: `hlc`, the class name, from, to, and the git oid (`Oid::None` when empty).
    PathMove(u64, String, CvPath, CvPath, Oid),
}

/// The `pathmove` class names ([F07 §2.2], [40 §2.4]).
const PATHMOVE_CLASSES: [&str; 4] = ["explicit", "confirmed", "committed", "observed"];

/// The element tags a set may hold ([F07 §7.1] tag 9).
const SET_ELEMS: [u8; 8] = [3, 6, 7, 10, 11, 12, 13, 14];

fn read_lp<'a>(r: &mut Reader<'a>) -> DResult<&'a [u8]> {
    let n = r.u32()? as usize;
    r.bytes(n)
}

fn read_lp_str<'a>(r: &mut Reader<'a>, what: &str) -> DResult<&'a str> {
    let at = r.offset();
    let b = read_lp(r)?;
    core::str::from_utf8(b).or_else(|_| err(at, format!("{what} is not UTF-8 [F07 §2.1]")))
}

fn read_cv_path(r: &mut Reader<'_>) -> DResult<CvPath> {
    let at = r.offset();
    let root = read_lp_str(r, "a path root name")?;
    if root.is_empty() {
        return err(at, "a path root name is empty [F07 §7.1 tag 12]");
    }
    let text = read_lp_str(r, "a path text")?;
    Ok(CvPath {
        root: root.to_owned(),
        text: text.to_owned(),
    })
}

/// An `oid` payload: `lp(algo name) ‖ lp(digest)`; both parts empty only where `empty_ok` (the `git` of a pathmove).
fn read_cv_oid(r: &mut Reader<'_>, empty_ok: bool) -> DResult<Oid> {
    let at = r.offset();
    let algo = read_lp(r)?;
    let digest = read_lp(r)?;
    Ok(match (algo, digest.len()) {
        (b"", 0) if empty_ok => Oid::None,
        (b"sha1", 20) => Oid::Sha1(digest.try_into().expect("20")),
        (b"sha256", 32) => Oid::Sha256(digest.try_into().expect("32")),
        _ => {
            return err(
                at,
                "an oid is not sha1 with 20 bytes or sha256 with 32 [F07 §7.1 tag 13]",
            );
        }
    })
}

fn write_cv_path(p: &CvPath, out: &mut Vec<u8>) {
    lp(out, p.root.as_bytes());
    lp(out, p.text.as_bytes());
}

fn write_cv_oid(o: &Oid, out: &mut Vec<u8>) {
    let name: &[u8] = match o {
        Oid::None => b"",
        Oid::Sha1(_) => b"sha1",
        Oid::Sha256(_) => b"sha256",
    };
    lp(out, name);
    lp(out, o.digest());
}

impl Cv {
    /// The canonical tag ([F07 §7.1]).
    pub fn tag(&self) -> u8 {
        match self {
            Cv::Absent => 0,
            Cv::Bool(false) => 1,
            Cv::Bool(true) => 2,
            Cv::Int(_) => 3,
            Cv::Counter(_) => 4,
            Cv::F64(_) => 5,
            Cv::Enum(_) => 6,
            Cv::Text(_) => 7,
            Cv::Set(..) => 9,
            Cv::Ref(_) => 10,
            Cv::CommitRef(_) => 11,
            Cv::Path(_) => 12,
            Cv::Oid(_) => 13,
            Cv::PathMove(..) => 14,
        }
    }

    /// Decodes one `cv`: the tag byte, then its payload, with the rules of [F07 §7.1] (empty is absent, the set rules).
    pub fn decode(r: &mut Reader<'_>) -> DResult<Cv> {
        let at = r.offset();
        let tag = r.u8()?;
        if tag == 9 {
            return Self::decode_set(r, at);
        }
        Self::decode_payload(r, tag, at)
    }

    fn decode_set(r: &mut Reader<'_>, at: usize) -> DResult<Cv> {
        let elem = r.u8()?;
        if !SET_ELEMS.contains(&elem) {
            return err(
                at + 1,
                format!("set element tag {elem} is not one of 3, 6, 7, 10-14 [F07 §7.1 tag 9]"),
            );
        }
        let n_at = r.offset();
        let n = r.u32()?;
        if n == 0 {
            return err(n_at, "a set with count 0 is absent, never tag 9 [F07 §7.1]");
        }
        let mut items = Vec::with_capacity((n as usize).min(r.remaining()));
        for _ in 0..n {
            let e_at = r.offset();
            items.push(Self::decode_payload(r, elem, e_at)?);
        }
        let payloads: Vec<Vec<u8>> = items.iter().map(Cv::payload).collect();
        if payloads.windows(2).any(|w| w[0] >= w[1]) {
            return err(
                n_at + 4,
                "set elements are not in strictly ascending bytewise order [F07 §2.4]",
            );
        }
        Ok(Cv::Set(elem, items))
    }

    fn decode_payload(r: &mut Reader<'_>, tag: u8, at: usize) -> DResult<Cv> {
        Ok(match tag {
            0 => Cv::Absent,
            1 => Cv::Bool(false),
            2 => Cv::Bool(true),
            3 => Cv::Int(r.i64()?),
            4 => Cv::Counter(r.i64()?),
            5 => {
                let f_at = r.offset();
                let bits = r.f64_bits()?;
                check_f64(bits, f_at)?;
                Cv::F64(bits)
            }
            6 => {
                let n_at = r.offset();
                let name = read_lp_str(r, "an enumeration name")?;
                if name.is_empty() {
                    return err(n_at, "an enumeration name is empty [F07 §2.2]");
                }
                Cv::Enum(name.to_owned())
            }
            7 => {
                let t_at = r.offset();
                let t = read_lp_str(r, "a text")?;
                if t.is_empty() {
                    return err(t_at, "the empty text is absent, never tag 7 [F07 §7.1]");
                }
                Cv::Text(t.to_owned())
            }
            10 => Cv::Ref(r.b16()?),
            11 => Cv::CommitRef(r.b32()?),
            12 => Cv::Path(read_cv_path(r)?),
            13 => Cv::Oid(read_cv_oid(r, false)?),
            14 => {
                let hlc = r.u64()?;
                let c_at = r.offset();
                let class = read_lp_str(r, "a pathmove class")?;
                if !PATHMOVE_CLASSES.contains(&class) {
                    return err(
                        c_at,
                        format!("pathmove class {class:?} is not a class name [F07 §2.2]"),
                    );
                }
                let from = read_cv_path(r)?;
                let to = read_cv_path(r)?;
                let git = read_cv_oid(r, true)?;
                Cv::PathMove(hlc, class.to_owned(), from, to, git)
            }
            _ => {
                return err(
                    at,
                    format!("canonical tag {tag} is unused or unknown [F07 §7.1]"),
                );
            }
        })
    }

    /// The payload without its tag ([F07 §7.1]; a set element is written this way).
    pub fn payload(&self) -> Vec<u8> {
        let mut o = Vec::new();
        match self {
            Cv::Absent | Cv::Bool(_) => {}
            Cv::Int(v) | Cv::Counter(v) => o.extend_from_slice(&v.to_le_bytes()),
            Cv::F64(b) => o.extend_from_slice(&b.to_le_bytes()),
            Cv::Enum(s) | Cv::Text(s) => lp(&mut o, s.as_bytes()),
            Cv::Set(elem, items) => {
                o.push(*elem);
                o.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for it in items {
                    o.extend_from_slice(&it.payload());
                }
            }
            Cv::Ref(u) => o.extend_from_slice(u),
            Cv::CommitRef(c) => o.extend_from_slice(c),
            Cv::Path(p) => write_cv_path(p, &mut o),
            Cv::Oid(x) => write_cv_oid(x, &mut o),
            Cv::PathMove(hlc, class, from, to, git) => {
                o.extend_from_slice(&hlc.to_le_bytes());
                lp(&mut o, class.as_bytes());
                write_cv_path(from, &mut o);
                write_cv_path(to, &mut o);
                write_cv_oid(git, &mut o);
            }
        }
        o
    }

    /// The tag and the payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = vec![self.tag()];
        o.extend_from_slice(&self.payload());
        o
    }
}

/// Steps 2–4 of [F07 §5.1] over valid text: CR LF and lone CR to LF, trailing HT/VT/FF/SP per line, trailing LF.
fn normalise_lines(s: &str) -> String {
    let unified = s.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unified
        .split('\n')
        .map(|l| l.trim_end_matches(['\t', '\u{0B}', '\u{0C}', ' ']))
        .collect();
    lines.join("\n").trim_end_matches('\n').to_owned()
}

/// Why N refuses a message ([F07 §5.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageRefusal {
    /// Not valid UTF-8, or holds U+0000.
    Encoding,
    /// Longer than 65,535 bytes after normalisation.
    TooLong,
    /// The last paragraph begins with `Moirai-`.
    TrailerLike,
}

/// N(m) of [F07 §5.1] with the refusals of §5.2.
pub fn normalise(m: &[u8]) -> Result<String, MessageRefusal> {
    let s = core::str::from_utf8(m).map_err(|_| MessageRefusal::Encoding)?;
    if s.contains('\0') {
        return Err(MessageRefusal::Encoding);
    }
    let n = normalise_lines(s);
    if n.len() > 65_535 {
        return Err(MessageRefusal::TooLong);
    }
    if last_paragraph(&n).starts_with("Moirai-") {
        return Err(MessageRefusal::TrailerLike);
    }
    Ok(n)
}

/// The last paragraph of a normalised message ([F07 §5.2]): the run of non-empty lines after the last empty line.
fn last_paragraph(n: &str) -> &str {
    let lines: Vec<&str> = n.split('\n').collect();
    match lines.iter().rposition(|l| l.is_empty()) {
        Some(i) => {
            let start: usize = lines[..=i].iter().map(|l| l.len() + 1).sum();
            &n[start.min(n.len())..]
        }
        None => n,
    }
}

/// N_imp of [F07 §5.3]: lossy UTF-8, U+0000 to U+FFFD, steps 2–4, the 65,535-byte cut; never refuses.
pub fn normalise_import(m: &[u8]) -> String {
    let s = String::from_utf8_lossy(m).replace('\0', "\u{FFFD}");
    let mut n = normalise_lines(&s);
    if n.len() > 65_535 {
        let mut cut = 65_535;
        while !n.is_char_boundary(cut) {
            cut -= 1;
        }
        n.truncate(cut);
        n = normalise_lines(&n);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [F07 §16]: the 196-byte commit input of the `claim --start` example (P… = 0x50 bytes, D… = 0x44 bytes).
    #[test]
    fn commit_input_example() {
        let it = Items {
            kind: "ordinary".into(),
            parents: vec![[0x50; 32]],
            hlc: 0x01A0_C450_6C00_0003,
            actor: "dev#1".into(),
            role: "developer".into(),
            session: "claude:s1".into(),
            git_algo: String::new(),
            git_head: vec![],
            git_branch: String::new(),
            git_worktree: String::new(),
            git_base: vec![],
            message: "claim --start".into(),
            schema_version: 1,
            origin: None,
            foreign: Oid::None,
            changeset_digest: [0x44; 32],
        };
        let c = it.input();
        assert_eq!(c.len(), 196);
        assert_eq!(&c[..20], b"\x10\x00\x00\x00moirai-commit-v1");
        assert_eq!(&c[20..32], b"\x08\x00\x00\x00ordinary");
        assert_eq!(it.commit_id(), blake3_256(&c));
    }

    /// [F07 §16]: the 74-byte digest input of one 43-byte status entry.
    #[test]
    fn changeset_digest_example() {
        let mut e = vec![0x01];
        e.extend_from_slice(&crate::prim::unhex("00112233445566778899aabbccddeeff").unwrap());
        e.extend_from_slice(&[0x02, 0x00, 0x01]);
        lp(&mut e, b"in_progress");
        lp(&mut e, b"none");
        assert_eq!(e.len(), 43);
        let mut input = Vec::new();
        lp(&mut input, b"moirai-changeset-v1");
        input.extend_from_slice(&e);
        input.extend_from_slice(&1u64.to_le_bytes());
        assert_eq!(input.len(), 74);
        assert_eq!(changeset_digest(&[e]), blake3_256(&input));
    }

    /// [F07 §5.4] the message examples.
    #[test]
    fn message_examples() {
        assert_eq!(
            normalise(b"fix lock\r\n\r\nsee #12  \r\n\r\n\r\n").unwrap(),
            "fix lock\n\nsee #12"
        );
        assert_eq!(normalise(b"a\rb\t\n").unwrap(), "a\nb");
        assert_eq!(normalise(b"\n\nsubject").unwrap(), "\n\nsubject");
        assert_eq!(normalise(b" \t\n").unwrap(), "");
        assert_eq!(
            normalise(b"done\n\nMoirai-Ref: main"),
            Err(MessageRefusal::TrailerLike)
        );
        assert_eq!(
            normalise(b"done\n\nnote\nMoirai-Ref: main").unwrap(),
            "done\n\nnote\nMoirai-Ref: main"
        );
        assert_eq!(normalise(b"a\0b"), Err(MessageRefusal::Encoding));
        assert_eq!(normalise_import(b"x\xFFy\0\r\n"), "x\u{FFFD}y\u{FFFD}");
    }

    fn cv(b: &[u8]) -> DResult<Cv> {
        let mut r = Reader::new(b);
        let v = Cv::decode(&mut r)?;
        r.finish("a cv")?;
        Ok(v)
    }

    /// [F07 §7.1]: tags, payloads, and the refusals (empty is absent, tag 8 unused, the set rules, −0.0).
    #[test]
    fn cv_rules() {
        assert_eq!(cv(&[0]).unwrap(), Cv::Absent);
        assert_eq!(cv(&[2]).unwrap(), Cv::Bool(true));
        let mut int = vec![3];
        int.extend_from_slice(&(-3i64).to_le_bytes());
        assert_eq!(cv(&int).unwrap(), Cv::Int(-3));
        assert!(cv(&[8]).is_err());
        assert!(cv(&[7, 0, 0, 0, 0]).is_err());
        assert!(cv(&[9, 3, 0, 0, 0, 0]).is_err());
        assert!(cv(&[9, 1, 1, 0, 0, 0]).is_err());
        let mut neg0 = vec![5];
        neg0.extend_from_slice(&0x8000_0000_0000_0000u64.to_le_bytes());
        assert!(cv(&neg0).is_err());
        let set = Cv::Set(7, vec![Cv::Text("a".into()), Cv::Text("b".into())]);
        assert_eq!(cv(&set.encode()).unwrap(), set);
        let unsorted = Cv::Set(7, vec![Cv::Text("b".into()), Cv::Text("a".into())]);
        assert!(cv(&unsorted.encode()).is_err());
        let path = |t: &str| CvPath {
            root: "project".into(),
            text: t.into(),
        };
        let mv = Cv::PathMove(7, "explicit".into(), path("a/"), path("b/"), Oid::None);
        assert_eq!(cv(&mv.encode()).unwrap(), mv);
        assert!(cv(&Cv::Oid(Oid::None).encode()).is_err());
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn path() -> impl Strategy<Value = CvPath> {
        ("[a-z]{1,8}", "[a-z/.]{0,12}").prop_map(|(root, text)| CvPath { root, text })
    }

    fn oid() -> impl Strategy<Value = Oid> {
        prop_oneof![
            any::<[u8; 20]>().prop_map(Oid::Sha1),
            any::<[u8; 32]>().prop_map(Oid::Sha256),
        ]
    }

    fn scalar() -> impl Strategy<Value = Cv> {
        prop_oneof![
            Just(Cv::Absent),
            any::<bool>().prop_map(Cv::Bool),
            any::<i64>().prop_map(Cv::Int),
            any::<i64>().prop_map(Cv::Counter),
            any::<f64>()
                .prop_filter("finite, not -0.0", |x| {
                    x.is_finite() && x.to_bits() != 0x8000_0000_0000_0000
                })
                .prop_map(|x| Cv::F64(x.to_bits())),
            "[a-z_]{1,10}".prop_map(Cv::Enum),
            "[^\u{0}]{1,20}".prop_map(Cv::Text),
            any::<[u8; 16]>().prop_map(Cv::Ref),
            any::<[u8; 32]>().prop_map(Cv::CommitRef),
            path().prop_map(Cv::Path),
            oid().prop_map(Cv::Oid),
            (
                any::<u64>(),
                proptest::sample::select(&PATHMOVE_CLASSES[..]),
                path(),
                path(),
                prop_oneof![Just(Oid::None), oid()],
            )
                .prop_map(|(h, c, f, t, g)| Cv::PathMove(h, c.to_owned(), f, t, g)),
        ]
    }

    proptest! {
        /// [F07 §7.1]: every `cv` a writer can form decodes back to itself, and a set of distinct int elements in the
        /// order of §2.4 round-trips.
        #[test]
        fn cv_round_trip(v in scalar(), ints in proptest::collection::btree_set(any::<i64>(), 1..6)) {
            let b = v.encode();
            let mut r = Reader::new(&b);
            prop_assert_eq!(Cv::decode(&mut r).unwrap(), v);
            prop_assert!(r.is_empty());
            let mut items: Vec<Cv> = ints.into_iter().map(Cv::Int).collect();
            items.sort_by_key(Cv::payload);
            let set = Cv::Set(3, items);
            let b = set.encode();
            prop_assert_eq!(Cv::decode(&mut Reader::new(&b)).unwrap(), set);
        }

        /// The decoder refuses or accepts any bytes without panicking.
        #[test]
        fn cv_never_panics(b in proptest::collection::vec(any::<u8>(), 0..64)) {
            let _ = Cv::decode(&mut Reader::new(&b));
        }
    }
}
