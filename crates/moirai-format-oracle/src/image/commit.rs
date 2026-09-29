//! [F14 §10] commit objects of the image: their bytes (§10.1), author and committer (§10.2), the message and trailer
//! block (§10.3), the trailers (§10.4, §10.5), classification of an imported commit (§10.9); and the gate-0 carrier check
//! of §12.1: the canonical items 1–9 re-derived from the carriers ([F07 §12.2]–§12.4), item 10 taken as given, and the
//! commit id recomputed ([F07 §3.1]).

use super::text::*;
use crate::canon::{Items, normalise_import};
use crate::prim::{Algo, Oid, Result, hex};

/// A git identity line's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    /// The name bytes.
    pub name: Vec<u8>,
    /// The email bytes (between `<` and `>`).
    pub email: Vec<u8>,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// The zone text (`+0000`).
    pub tz: String,
}

/// A git commit object's content ([F14 §10.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitObj {
    /// `tree`.
    pub tree: Oid,
    /// `parent` lines in order.
    pub parents: Vec<Oid>,
    /// `author`.
    pub author: Ident,
    /// `committer`.
    pub committer: Ident,
    /// Every other header line, verbatim (a foreign commit may carry `encoding`, `gpgsig`, …).
    pub extra: Vec<Vec<u8>>,
    /// The message bytes.
    pub message: Vec<u8>,
}

fn parse_ident(b: &[u8], at: usize) -> Result<Ident> {
    let lt = b.iter().position(|&c| c == b'<');
    let gt = b.iter().rposition(|&c| c == b'>');
    let (Some(lt), Some(gt)) = (lt, gt) else {
        return parse_err(
            Rule::GitObject,
            at,
            "a git identity lacks <email> [git-objects]",
        );
    };
    if lt == 0 || b[lt - 1] != b' ' || gt < lt || b.get(gt + 1) != Some(&b' ') {
        return parse_err(
            Rule::GitObject,
            at,
            "a git identity breaks `name <email> time zone` [git-objects]",
        );
    }
    let tail = core::str::from_utf8(&b[gt + 2..])
        .or_else(|_| parse_err(Rule::GitObject, at, "a git identity's time is not ASCII"))?;
    let Some((t, tz)) = tail.split_once(' ') else {
        return parse_err(
            Rule::GitObject,
            at,
            "a git identity lacks its zone [git-objects]",
        );
    };
    let Ok(time) = t.parse::<i64>() else {
        return parse_err(
            Rule::GitObject,
            at,
            "a git identity's time is not a decimal [git-objects]",
        );
    };
    Ok(Ident {
        name: b[..lt - 1].to_vec(),
        email: b[lt + 1..gt].to_vec(),
        time,
        tz: tz.to_owned(),
    })
}

fn ident_bytes(i: &Ident) -> Vec<u8> {
    let mut o = i.name.clone();
    o.extend_from_slice(b" <");
    o.extend_from_slice(&i.email);
    o.extend_from_slice(format!("> {} {}", i.time, i.tz).as_bytes());
    o
}

fn parse_oidhex(s: &[u8], algo: Algo, at: usize) -> Result<Oid> {
    let t = core::str::from_utf8(s).unwrap_or("");
    if !is_lhex(t, 2 * algo.digest_len()) {
        return parse_err(
            Rule::GitObject,
            at,
            "an object id is not lower-case hex of the destination's format [F14 §10.1]",
        );
    }
    let d = crate::prim::unhex(t).expect("hex");
    Ok(match algo {
        Algo::Sha1 => Oid::Sha1(d.try_into().expect("20")),
        Algo::Sha256 => Oid::Sha256(d.try_into().expect("32")),
    })
}

/// Parses a git commit object's content for a destination of object format `algo`.
pub fn parse_commit_object(b: &[u8], algo: Algo) -> Result<CommitObj> {
    let Some(sep) = b.windows(2).position(|w| w == b"\n\n") else {
        return parse_err(
            Rule::GitObject,
            0,
            "a commit object has no empty line before its message [git-objects]",
        );
    };
    let head = &b[..sep];
    let message = b[sep + 2..].to_vec();
    let mut tree = None;
    let mut parents = Vec::new();
    let (mut author, mut committer) = (None, None);
    let mut extra: Vec<Vec<u8>> = Vec::new();
    let mut off = 0;
    for line in head.split(|&c| c == b'\n') {
        if let Some(v) = line.strip_prefix(b"tree ") {
            if tree.is_some() || !parents.is_empty() || author.is_some() {
                return parse_err(
                    Rule::GitObject,
                    off,
                    "the tree line is not first or repeats [git-objects]",
                );
            }
            tree = Some(parse_oidhex(v, algo, off)?);
        } else if let Some(v) = line.strip_prefix(b"parent ") {
            if tree.is_none() || author.is_some() {
                return parse_err(
                    Rule::GitObject,
                    off,
                    "a parent line out of place [git-objects]",
                );
            }
            parents.push(parse_oidhex(v, algo, off)?);
        } else if let Some(v) = line.strip_prefix(b"author ") {
            if author.is_some() || tree.is_none() {
                return parse_err(
                    Rule::GitObject,
                    off,
                    "an author line out of place [git-objects]",
                );
            }
            author = Some(parse_ident(v, off)?);
        } else if let Some(v) = line.strip_prefix(b"committer ") {
            if committer.is_some() || author.is_none() {
                return parse_err(
                    Rule::GitObject,
                    off,
                    "a committer line out of place [git-objects]",
                );
            }
            committer = Some(parse_ident(v, off)?);
        } else if committer.is_some() {
            extra.push(line.to_vec());
        } else {
            return parse_err(
                Rule::GitObject,
                off,
                "an unknown header before committer [git-objects]",
            );
        }
        off += line.len() + 1;
    }
    let (Some(tree), Some(author), Some(committer)) = (tree, author, committer) else {
        return parse_err(
            Rule::GitObject,
            0,
            "a commit object lacks tree, author or committer [git-objects]",
        );
    };
    Ok(CommitObj {
        tree,
        parents,
        author,
        committer,
        extra,
        message,
    })
}

/// Re-encodes a commit object's content.
pub fn encode_commit_object(c: &CommitObj) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(format!("tree {}\n", hex(c.tree.digest())).as_bytes());
    for p in &c.parents {
        o.extend_from_slice(format!("parent {}\n", hex(p.digest())).as_bytes());
    }
    o.extend_from_slice(b"author ");
    o.extend_from_slice(&ident_bytes(&c.author));
    o.extend_from_slice(b"\ncommitter ");
    o.extend_from_slice(&ident_bytes(&c.committer));
    o.push(b'\n');
    for x in &c.extra {
        o.extend_from_slice(x);
        o.push(b'\n');
    }
    o.push(b'\n');
    o.extend_from_slice(&c.message);
    o
}

/// [F14 §10.3]: (message part, final paragraph lines).
pub fn split_message(m: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut lines: Vec<&[u8]> = m.split(|&c| c == b'\n').collect();
    if lines.last() == Some(&&b""[..]) {
        lines.pop();
    }
    match lines.iter().rposition(|l| l.is_empty()) {
        Some(i) => (
            lines[..i].join(&b'\n'),
            lines[i + 1..].iter().map(|l| l.to_vec()).collect(),
        ),
        None => (Vec::new(), lines.iter().map(|l| l.to_vec()).collect()),
    }
}

/// The trailer keys in exporter order ([F14 §10.5]).
pub const TRAILER_ORDER: [&str; 20] = [
    "Moirai-Commit",
    "Moirai-Kind",
    "Moirai-Parent",
    "Moirai-Head",
    "Moirai-Ref",
    "Moirai-Folded",
    "Moirai-Hlc",
    "Moirai-Actor",
    "Moirai-Role",
    "Moirai-Session",
    "Moirai-Git-Head",
    "Moirai-Git-Branch",
    "Moirai-Worktree",
    "Moirai-Git-Base",
    "Moirai-Origin",
    "Moirai-Sync-Base",
    "Moirai-Foreign-Git",
    "Moirai-Schema",
    "Moirai-Ops",
    "Moirai-Idem",
];

/// A `Moirai-Folded` value ([F14 §10.7]): the commit count `n` and, when `n >= 1`, the first and last folded commit ids.
pub type Folded = (u64, Option<([u8; 32], [u8; 32])>);

/// The parsed trailers of a native or checkpoint commit ([F14 §10.4]).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Trailers {
    /// `Moirai-Commit`.
    pub commit: Option<[u8; 32]>,
    /// `Moirai-Kind`.
    pub kind: Option<String>,
    /// `Moirai-Parent` by index 1 and 2.
    pub parent: [Option<[u8; 32]>; 2],
    /// `Moirai-Head`.
    pub head: Option<[u8; 32]>,
    /// `Moirai-Ref`.
    pub ref_name: Option<String>,
    /// `Moirai-Folded`: (n, first, last).
    pub folded: Option<Folded>,
    /// `Moirai-Hlc`.
    pub hlc: Option<u64>,
    /// `Moirai-Actor`.
    pub actor: Option<String>,
    /// `Moirai-Role`.
    pub role: Option<String>,
    /// `Moirai-Session`.
    pub session: Option<String>,
    /// `Moirai-Git-Head`.
    pub git_head: Option<Oid>,
    /// `Moirai-Git-Branch`.
    pub git_branch: Option<String>,
    /// `Moirai-Worktree`.
    pub worktree: Option<String>,
    /// `Moirai-Git-Base`.
    pub git_base: Option<Oid>,
    /// `Moirai-Origin`.
    pub origin: Option<[u8; 32]>,
    /// `Moirai-Sync-Base`.
    pub sync_base: Option<[u8; 32]>,
    /// `Moirai-Foreign-Git`.
    pub foreign: Option<Oid>,
    /// `Moirai-Schema`.
    pub schema: Option<u64>,
    /// `Moirai-Ops`.
    pub ops: Option<u64>,
    /// `Moirai-Idem`.
    pub idem: Option<[u8; 16]>,
    /// The keys in the order they appeared.
    pub order: Vec<String>,
}

/// How an imported git commit is classified ([F14 §10.9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Class {
    /// A native candidate.
    Native(Box<Trailers>),
    /// A checkpoint candidate.
    Checkpoint(Box<Trailers>),
    /// Foreign.
    Foreign,
}

fn trailer_value(t: &mut Trailers, key: &str, v: &str, at: usize) -> Result<()> {
    let bad = || {
        parse_err(
            Rule::TrailerMalformed,
            at,
            format!("a malformed value of trailer {key}: {v:?} [F14 §9.2]"),
        )
    };
    let cid = |s: &str| parse_commit_id(s);
    macro_rules! set {
        ($field:expr, $val:expr) => {{
            if $field.is_some() {
                return parse_err(
                    Rule::TrailerRepeated,
                    at,
                    format!("trailer {key} repeated [F14 §9.2]"),
                );
            }
            $field = Some($val);
        }};
    }
    match key {
        "Moirai-Commit" => match cid(v) {
            Some(c) => set!(t.commit, c),
            None => return bad(),
        },
        "Moirai-Kind" => {
            if ![
                "ordinary",
                "merge",
                "sync",
                "revert",
                "cherry-pick",
                "checkpoint",
            ]
            .contains(&v)
            {
                return bad();
            }
            set!(t.kind, v.to_owned())
        }
        "Moirai-Parent" => {
            let (i, c) = match v.split_once(' ') {
                Some(("1", c)) => (0, c),
                Some(("2", c)) => (1, c),
                _ => return bad(),
            };
            match cid(c) {
                Some(c) => set!(t.parent[i], c),
                None => return bad(),
            }
        }
        "Moirai-Head" => match cid(v) {
            Some(c) => set!(t.head, c),
            None => return bad(),
        },
        "Moirai-Ref" => {
            if v.is_empty() || v.contains(' ') || v.chars().any(is_control) {
                return bad();
            }
            set!(t.ref_name, v.to_owned())
        }
        "Moirai-Folded" => {
            let Some((n, rest)) = v.split_once(" commits") else {
                return bad();
            };
            let Some(n) = parse_dec(n) else {
                return bad();
            };
            let range = if rest.is_empty() {
                None
            } else {
                let Some((a, b)) = rest
                    .strip_prefix(" from ")
                    .and_then(|r| r.split_once(" to "))
                else {
                    return bad();
                };
                match (cid(a), cid(b)) {
                    (Some(a), Some(b)) => Some((a, b)),
                    _ => return bad(),
                }
            };
            if (n == 0) != range.is_none() {
                return parse_err(
                    Rule::TrailerMalformed,
                    at,
                    "Moirai-Folded names a range exactly when n >= 1 [F14 §10.7]",
                );
            }
            set!(t.folded, (n, range))
        }
        "Moirai-Hlc" => match parse_dec(v) {
            Some(h) => set!(t.hlc, h),
            None => return bad(),
        },
        "Moirai-Actor" | "Moirai-Role" | "Moirai-Session" | "Moirai-Git-Branch"
        | "Moirai-Worktree" => {
            let s = read_sval(v, at)?;
            match key {
                "Moirai-Actor" => set!(t.actor, s),
                "Moirai-Role" => set!(t.role, s),
                "Moirai-Session" => set!(t.session, s),
                "Moirai-Git-Branch" => set!(t.git_branch, s),
                _ => set!(t.worktree, s),
            }
        }
        "Moirai-Git-Head" | "Moirai-Git-Base" | "Moirai-Foreign-Git" => {
            let Some(o) = parse_oid_text(v) else {
                return bad();
            };
            match key {
                "Moirai-Git-Head" => set!(t.git_head, o),
                "Moirai-Git-Base" => set!(t.git_base, o),
                _ => set!(t.foreign, o),
            }
        }
        "Moirai-Origin" => match cid(v) {
            Some(c) => set!(t.origin, c),
            None => return bad(),
        },
        "Moirai-Sync-Base" => match cid(v) {
            Some(c) => set!(t.sync_base, c),
            None => return bad(),
        },
        "Moirai-Schema" => match parse_dec(v) {
            Some(s) => set!(t.schema, s),
            None => return bad(),
        },
        "Moirai-Ops" => match parse_dec(v) {
            Some(s) => set!(t.ops, s),
            None => return bad(),
        },
        "Moirai-Idem" => {
            if !is_lhex(v, 32) {
                return bad();
            }
            set!(
                t.idem,
                crate::prim::unhex(v).expect("hex").try_into().expect("16")
            )
        }
        _ => unreachable!("known keys only"),
    }
    t.order.push(key.to_owned());
    Ok(())
}

/// Classifies a git message ([F14 §10.9]) and parses a candidate's trailers; a malformed value, a repeat, an
/// inconsistency of §9.2 or a schema version other than 1 is `ImageParse`.
pub fn classify(message: &[u8]) -> Result<Class> {
    let (_, para) = split_message(message);
    let Some(first) = para.first() else {
        return Ok(Class::Foreign);
    };
    let native = first.starts_with(b"Moirai-Commit: ");
    let ckpt = first == b"Moirai-Kind: checkpoint"
        && !para.iter().any(|l| l.starts_with(b"Moirai-Commit:"));
    if !native && !ckpt {
        return Ok(Class::Foreign);
    }
    // Every line must be `Key: value` of a known key, or the commit is foreign; the key is read from the raw bytes, so a
    // known trailer whose value is not UTF-8 is a malformed value (ImageParse, §9.2), not an unknown line.
    let mut kv: Vec<(&str, &[u8])> = Vec::with_capacity(para.len());
    for l in &para {
        let known = l.windows(2).position(|w| w == b": ").and_then(|i| {
            let k = core::str::from_utf8(&l[..i]).ok()?;
            TRAILER_ORDER
                .iter()
                .find(|t| **t == k)
                .map(|t| (*t, &l[i + 2..]))
        });
        match known {
            Some(x) => kv.push(x),
            None => return Ok(Class::Foreign),
        }
    }
    let mut t = Trailers::default();
    for (i, (k, v)) in kv.into_iter().enumerate() {
        let Ok(v) = core::str::from_utf8(v) else {
            return parse_err(
                Rule::TrailerMalformed,
                i,
                format!("a malformed value of trailer {k}: not UTF-8 [F14 §9.2, §10.9]"),
            );
        };
        trailer_value(&mut t, k, v, i)?;
    }
    if let (Some(h), Some(b)) = (&t.git_head, &t.git_base)
        && h.algo_byte() != b.algo_byte()
    {
        return parse_err(
            Rule::TrailerAlgorithms,
            0,
            "Moirai-Git-Head and Moirai-Git-Base of different algorithms [F14 §9.2]",
        );
    }
    if t.schema.is_some_and(|s| s != 1) {
        return parse_err(
            Rule::TrailerSchema,
            0,
            "a schema version other than 1 [F14 §9.2]",
        );
    }
    if native {
        let need = t.kind.is_some() && t.hlc.is_some() && t.schema.is_some() && t.ops.is_some();
        let ck = t.kind.as_deref() == Some("checkpoint");
        if !need || (ck && (t.head.is_none() || t.folded.is_none() || t.foreign.is_none())) {
            return Ok(Class::Foreign);
        }
        Ok(Class::Native(Box::new(t)))
    } else {
        let only = t.order.iter().all(|k| {
            ["Moirai-Kind", "Moirai-Head", "Moirai-Ref", "Moirai-Folded"].contains(&k.as_str())
        });
        if !only || t.head.is_none() || t.folded.is_none() {
            return Ok(Class::Foreign);
        }
        Ok(Class::Checkpoint(Box::new(t)))
    }
}

/// What the carrier check knows about one git parent ([F14 §12.1] row 2): the id this store holds for it through
/// `gitmap` (a demoted parent's foreign id), its own `Moirai-Commit` when it is a native candidate, and its `hlc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParentInfo {
    /// The id the store holds for the git parent.
    pub id: [u8; 32],
    /// The parent's `Moirai-Commit` trailer, when it has one: a native child states it ([F14 §10.9]: children of a
    /// demoted commit still verify against the ids their trailers state).
    pub commit: Option<[u8; 32]>,
    /// Its `hlc` (for the foreign `hlc` rule).
    pub hlc: u64,
}

/// [F06 §4.4.4]: the deterministic `hlc` of a foreign or import-checkpoint commit.
pub fn foreign_hlc(committer_time: i64, parents: &[ParentInfo]) -> Result<u64> {
    if committer_time < 0 || (committer_time as u128) * 1000 >= 1 << 48 {
        return parse_err(
            Rule::NumberRange,
            0,
            "a committer time negative or at or beyond 2^48 ms [F06 §4.4.4]",
        );
    }
    let mut h = ((committer_time as u64) * 1000) << 16;
    for p in parents {
        let Some(n) = p.hlc.checked_add(1) else {
            return parse_err(
                Rule::NumberRange,
                0,
                "a parent's hlc + 1 exceeds 2^64 - 1 [F06 §4.4.4]",
            );
        };
        h = h.max(n);
    }
    Ok(h)
}

/// The inputs of the carrier check that are not in the commit object ([F14 §12.6]'s fixture contents).
#[derive(Clone, Debug)]
pub struct CarrierCtx {
    /// Each git parent's derived id and hlc, in order.
    pub parents: Vec<ParentInfo>,
    /// The tree marker's schema version ([F14 §4]).
    pub marker_schema_version: u32,
    /// The destination's object format.
    pub algo: Algo,
    /// The commit's own git object id (items 9 of foreign and checkpoint commits).
    pub own_oid: Oid,
    /// Item 10: the `changeset_digest` of the tree diff.
    pub changeset_digest: [u8; 32],
    /// The number of item-10 entries of the tree diff ([F07 §10.4]), which `Moirai-Ops` states ([F14 §12.1]).
    pub entry_count: u64,
}

/// [F14 §12.1]: `Moirai-Ops` carries the item-10 entry count as a pre-check; an importer that counts another number
/// demotes the commit without hashing ([F07 §12.5]).
fn ops_match(t: &Trailers, cx: &CarrierCtx) -> bool {
    t.ops == Some(cx.entry_count)
}

/// The parent count of [F07 §12.3] and the carrier context's agreement with the commit object.
fn check_context(obj: &CommitObj, cx: &CarrierCtx) -> Result<()> {
    if obj.parents.len() > 2 {
        return parse_err(
            Rule::ForeignParents,
            0,
            "a git commit with more than two parents [F07 §12.3]",
        );
    }
    if cx.parents.len() != obj.parents.len() {
        return parse_err(
            Rule::Context,
            0,
            "the carrier context names another number of parents than the commit",
        );
    }
    Ok(())
}

/// Items 1–10 of a native candidate from its trailers ([F07 §12.2], [F14 §12.1]), or `None` when its message part
/// fails step 1 of N ([F07 §5.3]: the commit is demoted). The other steps of N apply; a refusal of §5.2 does not demote
/// (the message part keeps N_imp of it; `fixtures/carrier/INDEX.md` C-1).
fn native_items(obj: &CommitObj, t: &Trailers, cx: &CarrierCtx) -> Result<Option<Items>> {
    let parents: Vec<[u8; 32]> = cx
        .parents
        .iter()
        .enumerate()
        .map(|(i, p)| {
            t.parent
                .get(i)
                .copied()
                .flatten()
                .or(p.commit)
                .unwrap_or(p.id)
        })
        .collect();
    if t.parent[1].is_some() && parents.len() < 2 || t.parent[0].is_some() && parents.is_empty() {
        return parse_err(
            Rule::TrailerMalformed,
            0,
            "Moirai-Parent names a parent the commit does not have [F14 §10.4]",
        );
    }
    if let Some(sb) = t.sync_base
        && parents.get(1) != Some(&sb)
    {
        return parse_err(
            Rule::TrailerSyncBase,
            0,
            "Moirai-Sync-Base differs from the second parent's stated id [F14 §9.2]",
        );
    }
    if t.schema != Some(u64::from(cx.marker_schema_version)) {
        return parse_err(
            Rule::TrailerSchema,
            0,
            "Moirai-Schema differs from the tree's marker [F14 §4]",
        );
    }
    let (msg_part, _) = split_message(&obj.message);
    let message = match crate::canon::normalise(&msg_part) {
        Ok(m) => m,
        Err(crate::canon::MessageRefusal::Encoding) => return Ok(None),
        Err(_) => normalise_import(&msg_part),
    };
    let git_algo = t
        .git_head
        .or(t.git_base)
        .map(|o| if o.algo_byte() == 1 { "sha1" } else { "sha256" });
    Ok(Some(Items {
        kind: t.kind.clone().expect("required"),
        parents,
        hlc: t.hlc.expect("required"),
        actor: t.actor.clone().unwrap_or_default(),
        role: t.role.clone().unwrap_or_default(),
        session: t.session.clone().unwrap_or_default(),
        git_algo: git_algo.unwrap_or("").to_owned(),
        git_head: t.git_head.map(|o| o.digest().to_vec()).unwrap_or_default(),
        git_branch: t.git_branch.clone().unwrap_or_default(),
        git_worktree: t.worktree.clone().unwrap_or_default(),
        git_base: t.git_base.map(|o| o.digest().to_vec()).unwrap_or_default(),
        message,
        schema_version: t.schema.expect("required") as u32,
        origin: t.origin,
        foreign: t.foreign.unwrap_or(Oid::None),
        changeset_digest: cx.changeset_digest,
    }))
}

/// Re-derives canonical items 1–10 of an image commit from its carriers ([F14 §12.1], [F07 §12.2]–§12.4).
pub fn derive_items(obj: &CommitObj, cx: &CarrierCtx) -> Result<(Class, Items)> {
    let class = classify(&obj.message)?;
    check_context(obj, cx)?;
    let items = match &class {
        Class::Native(t) => match native_items(obj, t, cx)? {
            Some(_) if !ops_match(t, cx) => {
                return parse_err(
                    Rule::Demoted,
                    0,
                    "Moirai-Ops differs from the item-10 entry count: the commit is demoted [F14 §12.1]",
                );
            }
            Some(items) => items,
            None => {
                return parse_err(
                    Rule::Demoted,
                    0,
                    "the message part breaks N step 1: the commit is demoted [F14 §10.9]",
                );
            }
        },
        Class::Checkpoint(_) => {
            if obj.parents.len() > 1 {
                return parse_err(
                    Rule::ForeignParents,
                    0,
                    "a checkpoint commit with two parents [F07 §12.4]",
                );
            }
            let (msg_part, _) = split_message(&obj.message);
            Items {
                kind: "checkpoint".into(),
                parents: cx.parents.iter().map(|p| p.id).collect(),
                hlc: foreign_hlc(obj.committer.time, &cx.parents)?,
                actor: "image:checkpoint".into(),
                role: String::new(),
                session: String::new(),
                git_algo: String::new(),
                git_head: vec![],
                git_branch: String::new(),
                git_worktree: String::new(),
                git_base: vec![],
                message: normalise_import(&msg_part),
                schema_version: cx.marker_schema_version,
                origin: None,
                foreign: cx.own_oid,
                changeset_digest: cx.changeset_digest,
            }
        }
        Class::Foreign => foreign_items(obj, cx)?,
    };
    Ok((class, items))
}

/// Items 1–10 of a foreign commit ([F07 §12.3]): kind by the parent count, the parents' ids, the foreign `hlc`
/// ([F06 §4.4.4]), `git:<author email>`, N_imp of the whole message, the marker's schema version and the commit's own
/// git id as item 9.
pub fn foreign_items(obj: &CommitObj, cx: &CarrierCtx) -> Result<Items> {
    if obj.parents.len() > 2 {
        return parse_err(
            Rule::ForeignParents,
            0,
            "a git commit with more than two parents [F07 §12.3]",
        );
    }
    Ok(Items {
        kind: if obj.parents.len() == 2 {
            "merge"
        } else {
            "ordinary"
        }
        .into(),
        parents: cx.parents.iter().map(|p| p.id).collect(),
        hlc: foreign_hlc(obj.committer.time, &cx.parents)?,
        actor: format!(
            "git:{}",
            String::from_utf8_lossy(&obj.author.email).replace('\0', "\u{FFFD}")
        ),
        role: String::new(),
        session: String::new(),
        git_algo: String::new(),
        git_head: vec![],
        git_branch: String::new(),
        git_worktree: String::new(),
        git_base: vec![],
        message: normalise_import(&obj.message),
        schema_version: cx.marker_schema_version,
        origin: None,
        foreign: cx.own_oid,
        changeset_digest: cx.changeset_digest,
    })
}

/// How an import treats a git commit after [F14 §10.9] and the id check of §12.6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A native candidate whose rebuilt id equals `Moirai-Commit`.
    Native,
    /// A native candidate whose `Moirai-Ops` is not the entry count ([F14 §12.1]), whose rebuilt id differs, or whose
    /// message part fails N step 1: imported as foreign ([F07 §12.5]). Carries the failed native reconstruction when
    /// one was built (unhashed when `Moirai-Ops` decided).
    Demoted(Option<Box<Items>>),
    /// A checkpoint candidate ([F07 §12.4]).
    Checkpoint,
    /// No trailer paragraph a candidate needs ([F07 §12.3]).
    Foreign,
}

/// The items an import rebuilds for a git commit ([F14 §10.9], §12.1): a native candidate that verifies keeps its
/// native items; one that does not is demoted and gets the foreign items; `ImageParse` refusals propagate. A
/// `Moirai-Ops` that differs from the entry count demotes before the id is computed or compared ([F14 §12.1]); the
/// native reconstruction is still returned, unhashed, for diagnostics (a fixture may state its id).
pub fn import_items(obj: &CommitObj, cx: &CarrierCtx) -> Result<(Verdict, Items)> {
    let class = classify(&obj.message)?;
    match &class {
        Class::Native(t) => {
            check_context(obj, cx)?;
            match native_items(obj, t, cx)? {
                Some(items) if !ops_match(t, cx) => Ok((
                    Verdict::Demoted(Some(Box::new(items))),
                    foreign_items(obj, cx)?,
                )),
                Some(items) if Some(items.commit_id()) == t.commit => Ok((Verdict::Native, items)),
                Some(items) => Ok((
                    Verdict::Demoted(Some(Box::new(items))),
                    foreign_items(obj, cx)?,
                )),
                None => Ok((Verdict::Demoted(None), foreign_items(obj, cx)?)),
            }
        }
        Class::Checkpoint(_) => Ok((Verdict::Checkpoint, derive_items(obj, cx)?.1)),
        Class::Foreign => Ok((Verdict::Foreign, derive_items(obj, cx)?.1)),
    }
}

/// The gate-0 check of one native commit ([F14 §12.6]): the id rebuilt from the carriers equals `Moirai-Commit`, and
/// the exporter's bytes follow §10.1, §10.2 and §10.5. Returns the verified id.
pub fn verify_native(obj: &CommitObj, cx: &CarrierCtx) -> Result<[u8; 32]> {
    let (class, items) = derive_items(obj, cx)?;
    let Class::Native(t) = class else {
        return parse_err(
            Rule::Export,
            0,
            "the commit is not a native commit [F14 §10.9]",
        );
    };
    let id = items.commit_id();
    if Some(id) != t.commit {
        return parse_err(
            Rule::Export,
            0,
            format!(
                "the rebuilt commit id c{} differs from Moirai-Commit [F14 §12.6]",
                hex(&id)
            ),
        );
    }
    check_exporter_bytes(obj, &t, &items)?;
    Ok(id)
}

/// [F14 §10.1], §10.2, §10.5: the bytes an exporter writes for a native commit.
pub fn check_exporter_bytes(obj: &CommitObj, t: &Trailers, items: &Items) -> Result<()> {
    if !obj.extra.is_empty() {
        return parse_err(
            Rule::Export,
            0,
            "a native commit carries a header other than tree, parent, author, committer [F14 §10.1]",
        );
    }
    let sub = |s: &str| -> Vec<u8> {
        s.bytes()
            .map(|b| {
                if matches!(b, 0 | b'\n' | b'<' | b'>') {
                    b'_'
                } else {
                    b
                }
            })
            .collect()
    };
    let mut name = b"moirai/".to_vec();
    name.extend(sub(&items.actor));
    let mut email = sub(&items.role);
    email.extend_from_slice(b"@moirai.invalid");
    let want = Ident {
        name,
        email,
        time: ((items.hlc >> 16) / 1000) as i64,
        tz: "+0000".into(),
    };
    if obj.author != want || obj.committer != want {
        return parse_err(
            Rule::Export,
            0,
            "author or committer is not moirai/<actor> <role@moirai.invalid> at floor((hlc >> 16) / 1000) [F14 §10.2]",
        );
    }
    let rank = |k: &str| TRAILER_ORDER.iter().position(|x| *x == k).expect("known");
    if t.order.windows(2).any(|w| rank(&w[0]) > rank(&w[1])) {
        return parse_err(
            Rule::Export,
            0,
            "trailers out of the exporter order [F14 §10.5]",
        );
    }
    let empties = [&t.actor, &t.role, &t.session, &t.git_branch, &t.worktree];
    if empties.iter().any(|v| v.as_deref() == Some("")) {
        return parse_err(
            Rule::Export,
            0,
            "a trailer with an empty value is written [F14 §10.4]",
        );
    }
    let mut want_msg = items.message.clone().into_bytes();
    if !want_msg.is_empty() {
        want_msg.extend_from_slice(b"\n\n");
    }
    let mut trailer_block = Vec::new();
    let (_, para) = split_message(&obj.message);
    for l in &para {
        trailer_block.extend_from_slice(l);
        trailer_block.push(b'\n');
    }
    want_msg.extend_from_slice(&trailer_block);
    if obj.message != want_msg {
        return parse_err(
            Rule::Export,
            0,
            "the message is not N(m), an empty line and the trailer block [F14 §10.3]",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [F14 §17.6]: the native message classifies with every trailer; the checkpoint message classifies as checkpoint.
    #[test]
    fn example_messages() {
        let native = "claim --start\n\nMoirai-Commit: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\nMoirai-Kind: ordinary\nMoirai-Ref: lane/l5np\nMoirai-Hlc: 117336598351118336\nMoirai-Actor: dev#1\nMoirai-Role: developer\nMoirai-Session: claude:s1\nMoirai-Git-Head: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1\nMoirai-Git-Branch: lane-l5np\nMoirai-Worktree: D:/work/demo-lanes/l5np\nMoirai-Git-Base: sha1:355b6ed5b782f6c37af0af19ff53ee150bdc8736\nMoirai-Schema: 1\nMoirai-Ops: 1\nMoirai-Idem: 6653a45d135b95e5b4266fe14c279747\n";
        let Class::Native(t) = classify(native.as_bytes()).unwrap() else {
            panic!("native")
        };
        assert_eq!(t.hlc, Some(117_336_598_351_118_336));
        assert_eq!((117_336_598_351_118_336u64 >> 16) / 1000, 1_790_414_403);
        let ck = "checkpoint main\n\nMoirai-Kind: checkpoint\nMoirai-Head: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad\nMoirai-Ref: main\nMoirai-Folded: 12 commits from cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df to ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad\n";
        assert!(matches!(
            classify(ck.as_bytes()).unwrap(),
            Class::Checkpoint(_)
        ));
        assert_eq!(classify(b"fix typo\n").unwrap(), Class::Foreign);
        assert_eq!(classify(b"x\n\nMoirai-Commit: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\nMoirai-Unknown: 1\n").unwrap(), Class::Foreign);
        assert!(classify(b"x\n\nMoirai-Commit: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\nMoirai-Hlc: 1\nMoirai-Hlc: 2\n").is_err());
    }

    /// A native commit built by the exporter's rules verifies, and a tampered message demotes.
    #[test]
    fn native_round_trip() {
        let items = Items {
            kind: "ordinary".into(),
            parents: vec![[7; 32]],
            hlc: 117_336_598_351_118_336,
            actor: "dev#1".into(),
            role: "developer".into(),
            session: String::new(),
            git_algo: String::new(),
            git_head: vec![],
            git_branch: String::new(),
            git_worktree: String::new(),
            git_base: vec![],
            message: "claim --start".into(),
            schema_version: 1,
            origin: None,
            foreign: Oid::None,
            changeset_digest: [9; 32],
        };
        let id = items.commit_id();
        let msg = format!(
            "claim --start\n\nMoirai-Commit: c{}\nMoirai-Kind: ordinary\nMoirai-Hlc: 117336598351118336\nMoirai-Actor: dev#1\nMoirai-Role: developer\nMoirai-Schema: 1\nMoirai-Ops: 1\n",
            hex(&id)
        );
        let who = "moirai/dev#1 <developer@moirai.invalid> 1790414403 +0000";
        let obj_bytes = format!(
            "tree {}\nparent {}\nauthor {who}\ncommitter {who}\n\n{msg}",
            "ab".repeat(20),
            "cd".repeat(20)
        );
        let obj = parse_commit_object(obj_bytes.as_bytes(), Algo::Sha1).unwrap();
        assert_eq!(encode_commit_object(&obj), obj_bytes.as_bytes());
        let cx = CarrierCtx {
            parents: vec![ParentInfo {
                id: [7; 32],
                commit: None,
                hlc: 1,
            }],
            marker_schema_version: 1,
            algo: Algo::Sha1,
            own_oid: Oid::None,
            changeset_digest: [9; 32],
            entry_count: 1,
        };
        assert_eq!(verify_native(&obj, &cx).unwrap(), id);
        assert_eq!(import_items(&obj, &cx).unwrap().0, Verdict::Native);
        let mut bad = obj.clone();
        bad.message[0] = b'C';
        assert!(verify_native(&bad, &cx).is_err());
        // [F14 §12.1]: a Moirai-Ops that is not the entry count demotes before the id is compared, so even a commit whose
        // rebuilt id equals Moirai-Commit is demoted.
        let other = CarrierCtx {
            entry_count: 2,
            ..cx.clone()
        };
        let (v, items) = import_items(&obj, &other).unwrap();
        assert!(
            matches!(&v, Verdict::Demoted(Some(n)) if n.commit_id() == id),
            "{v:?}"
        );
        assert_eq!(items.kind, "ordinary");
        assert!(items.actor.starts_with("git:"));
        assert!(verify_native(&obj, &other).is_err());
    }

    /// [F14 §9.2], §10.9: a known trailer with a value that is not UTF-8 is `ImageParse`; a non-UTF-8 key is an unknown
    /// line, so the commit is foreign.
    #[test]
    fn non_utf8_trailer_values() {
        let head = b"x\n\nMoirai-Commit: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n".to_vec();
        let mut bad_value = head.clone();
        bad_value.extend_from_slice(b"Moirai-Actor: \xff\n");
        assert!(classify(&bad_value).is_err());
        let mut bad_key = head;
        bad_key.extend_from_slice(b"Moirai-\xffctor: a\n");
        assert_eq!(classify(&bad_key).unwrap(), Class::Foreign);
    }

    /// [F06 §4.4.4]: the foreign `hlc` and its refusals.
    #[test]
    fn foreign_hlc_rule() {
        assert_eq!(
            foreign_hlc(1_790_000_000, &[]).unwrap(),
            (1_790_000_000_000u64) << 16
        );
        let p = [ParentInfo {
            id: [0; 32],
            commit: None,
            hlc: u64::from(u32::MAX) << 32,
        }];
        assert_eq!(foreign_hlc(1, &p).unwrap(), (u64::from(u32::MAX) << 32) + 1);
        assert!(foreign_hlc(-1, &[]).is_err());
    }
}
