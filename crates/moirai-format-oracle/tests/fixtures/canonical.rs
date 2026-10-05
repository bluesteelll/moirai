//! `fixtures/canonical` (WP-21): the ids of every case, recomputed. A decoder never recomputes a commit id or a
//! `changeset_digest` (their correctness is a C-rule, [F06 §4.3] orders 2 and 38); the oracle recomputes them over the
//! fixtures that carry real ids, here and in `fixtures/carrier` ([F07 §12] intro; spec sync 2b S2B-F-14). Per case
//! (`fixtures/canonical/INDEX.md` §2.2, §4): `changeset-digest` is BLAKE3-256 of `digest-input`, which is
//! `lp("moirai-changeset-v1") ‖ E_1 ‖ … ‖ E_n ‖ u64(entry-count)` ([F07 §10.4]); `commit-id` is BLAKE3-256 of `c`
//! ([F07 §3.1]); and where the case gives a `commit` block, C rebuilt from its items 1–9 (INDEX.md §4.2, [F07 §3.2]–§3.10)
//! and the case's `changeset-digest` as item 10 is `c` byte for byte. The entries of `digest-input` recompute the
//! digest, and a stated `message-input-hex` normalises (`N`, [F07 §5.1]) to the commit's message. The message cases give
//! `N` or `N_imp` ([F07 §5]) of their input, and every `cv` case decodes to its end and re-encodes ([F07 §7.1]); the
//! other unit encodings of `values.cases` are item-10 parts that the reference model derives (WP-91).

use moirai_format_oracle::canon::{Cv, Items, MessageRefusal, normalise, normalise_import};
use moirai_format_oracle::fixture::{Framed, hex_block, parse_framed};
use moirai_format_oracle::image::text::read_jstring;
use moirai_format_oracle::prim::{Oid, Reader, blake3_256, unhex};

use super::common::{check_digest_parts, family, rel, run_all, text, walk};

/// One case of a `.cases` file.
struct Case {
    /// `<file>#<case id>`.
    name: String,
    /// Its directives.
    framed: Framed,
}

/// Every case of `fixtures/canonical/cases/*.cases`.
fn cases() -> Vec<Case> {
    let base = family("canonical");
    let mut out = Vec::new();
    for p in walk(&base.join("cases")) {
        if p.extension().is_none_or(|e| e != "cases") {
            continue;
        }
        let file = rel(&p, &base);
        let recs = parse_framed(&text(&p)).unwrap_or_else(|e| panic!("{file}: {e}"));
        for framed in recs {
            let id = framed.line("case").unwrap_or("(no case line)").to_owned();
            out.push(Case {
                name: format!("{file}#{id}"),
                framed,
            });
        }
    }
    out
}

/// A 32-byte id written as 64 hex digits.
fn id32(s: &str) -> Result<[u8; 32], String> {
    unhex(s)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("{s:?} is not 64 hex digits"))
}

/// A JSON string that is the whole value.
fn json(v: &str) -> Result<String, String> {
    let (s, n) = read_jstring(v, 0).map_err(|e| e.to_string())?;
    if n != v.len() {
        return Err(format!("bytes after the JSON string {v:?}"));
    }
    Ok(s)
}

/// An object id written `<algo> <hex>` (INDEX.md §4.2), or `-` for none.
fn oid(v: &str) -> Result<Oid, String> {
    if v == "-" {
        return Ok(Oid::None);
    }
    let (algo, h) = v
        .split_once(' ')
        .ok_or_else(|| format!("{v:?} is not <algo> <hex>"))?;
    let b = unhex(h).ok_or_else(|| format!("{h:?} is not hex"))?;
    match (algo, b.len()) {
        ("sha1", 20) => Ok(Oid::Sha1(b.try_into().expect("20"))),
        ("sha256", 32) => Ok(Oid::Sha256(b.try_into().expect("32"))),
        _ => Err(format!("{v:?} is not a sha1 or sha256 object id")),
    }
}

/// The name of an object id's format and its digest bytes (empty for none).
fn algo_digest(o: &Oid) -> (&'static str, Vec<u8>) {
    match o {
        Oid::None => ("", Vec::new()),
        Oid::Sha1(d) => ("sha1", d.to_vec()),
        Oid::Sha256(d) => ("sha256", d.to_vec()),
    }
}

/// Items 1–9 of a `commit` block (INDEX.md §4.2) with item 10 through `changeset_digest`. `git_algo` is derived as
/// [F07 §3.6] says: the format's name when `git-head` or `git-base` is present, empty otherwise.
fn items_of(block: &[String], changeset_digest: [u8; 32]) -> Result<Items, String> {
    let mut it = Items {
        kind: String::new(),
        parents: Vec::new(),
        hlc: 0,
        actor: String::new(),
        role: String::new(),
        session: String::new(),
        git_algo: String::new(),
        git_head: Vec::new(),
        git_branch: String::new(),
        git_worktree: String::new(),
        git_base: Vec::new(),
        message: String::new(),
        schema_version: 0,
        origin: None,
        foreign: Oid::None,
        changeset_digest,
    };
    let mut seen = Vec::new();
    for l in block.iter().filter(|l| !l.is_empty()) {
        let (k, v) = l
            .split_once(' ')
            .ok_or_else(|| format!("commit line {l:?} has no value"))?;
        if k != "parent" && seen.contains(&k) {
            return Err(format!("commit line {k} repeated"));
        }
        seen.push(k);
        match k {
            "kind" => it.kind = v.to_owned(),
            "parent" => it.parents.push(id32(v)?),
            "hlc" => {
                it.hlc = u64::from_str_radix(v, 16)
                    .ok()
                    .filter(|_| v.len() == 16)
                    .ok_or_else(|| format!("hlc {v:?} is not 16 hex digits"))?
            }
            "actor" => it.actor = json(v)?,
            "role" => it.role = json(v)?,
            "session" => it.session = json(v)?,
            "git-head" | "git-base" => {
                let (a, d) = algo_digest(&oid(v)?);
                if !a.is_empty() {
                    if !it.git_algo.is_empty() && it.git_algo != a {
                        return Err("git-head and git-base of different formats".into());
                    }
                    it.git_algo = a.to_owned();
                }
                if k == "git-head" {
                    it.git_head = d;
                } else {
                    it.git_base = d;
                }
            }
            "git-branch" => it.git_branch = json(v)?,
            "git-worktree" => it.git_worktree = json(v)?,
            "message" => it.message = json(v)?,
            "schema-version" => {
                it.schema_version = v
                    .parse()
                    .map_err(|_| format!("schema-version {v:?} is not a decimal"))?
            }
            "origin" => it.origin = if v == "-" { None } else { Some(id32(v)?) },
            "foreign" => it.foreign = oid(v)?,
            _ => return Err(format!("unknown commit line {k}")),
        }
    }
    for k in [
        "kind",
        "hlc",
        "actor",
        "role",
        "session",
        "message",
        "schema-version",
    ] {
        if !seen.contains(&k) {
            return Err(format!("the commit block has no {k} line"));
        }
    }
    Ok(it)
}

/// What a case asks of the oracle (`fixtures/canonical/INDEX.md` §2.2, §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum What {
    /// A commit: item 10's digest and the commit id; with a `commit` block, C rebuilt from items 1–9.
    Commit,
    /// `N` or `N_imp` of `input-hex` ([F07 §5]).
    Message,
    /// A `cv` unit encoding ([F07 §7.1]).
    Cv,
    /// Another unit encoding of [F07 §7.2]–§9 (class values, `cstate`, deltas, selector blocks, schema items): item 10's
    /// entries are the reference model's (WP-91), so the oracle checks only the stated length.
    Unit,
}

fn what(f: &Framed) -> What {
    match (f.line("function"), f.line("encoding")) {
        (Some(_), _) => What::Message,
        (None, Some("cv")) => What::Cv,
        (None, Some(_)) => What::Unit,
        (None, None) => What::Commit,
    }
}

/// A hex block by name, if present.
fn hex_of(f: &Framed, name: &str) -> Result<Option<Vec<u8>>, String> {
    f.block(name)
        .map(|b| hex_block(b).map_err(|e| format!("{name}: {e}")))
        .transpose()
}

/// A line that states a length in bytes, compared with `bytes`.
fn length_is(f: &Framed, name: &str, bytes: &[u8]) -> Result<(), String> {
    let n: usize = f
        .line(name)
        .ok_or_else(|| format!("no {name} line"))?
        .parse()
        .map_err(|_| format!("{name} is not a decimal"))?;
    if n != bytes.len() {
        return Err(format!(
            "{name} {n}, but the bytes are {} long",
            bytes.len()
        ));
    }
    Ok(())
}

/// An informative text line, when present, is the same bytes as a JSON string (INDEX.md §2.2).
fn text_is(f: &Framed, name: &str, bytes: &[u8]) -> Result<(), String> {
    match f.line(name) {
        Some(v) if json(v)?.as_bytes() != bytes => Err(format!("{name} is not the hex bytes")),
        _ => Ok(()),
    }
}

/// `N` ([F07 §5.1]–§5.2) or `N_imp` (§5.3) of `input-hex`: `output-hex` of `output-length` bytes, or the `refused`
/// class of [F19] `bad_value` (`N` only; `N_imp` never refuses).
fn check_message(f: &Framed) -> Result<(), String> {
    let input = hex_of(f, "input-hex")?.ok_or("no input-hex block")?;
    text_is(f, "input-text", &input)?;
    let got = match f.line("function") {
        Some("N") => normalise(&input).map_err(|r| match r {
            MessageRefusal::Encoding => "message-utf8",
            MessageRefusal::TooLong => "message-length",
            MessageRefusal::TrailerLike => "message-trailer",
        }),
        Some("N_imp") => Ok(normalise_import(&input)),
        other => return Err(format!("function {other:?} is not N or N_imp")),
    };
    match (got, f.line("refused"), hex_of(f, "output-hex")?) {
        (Ok(out), None, Some(want)) => {
            if out.as_bytes() != want {
                return Err(format!(
                    "the function gives {out:?}, not output-hex ({} bytes)",
                    want.len()
                ));
            }
            length_is(f, "output-length", &want)?;
            text_is(f, "output-text", &want)
        }
        (Err(class), Some(want), None) if class == want => Ok(()),
        (got, refused, _) => Err(format!(
            "the function gives {got:?}; the case states {}",
            refused.map_or("an output".to_owned(), |r| format!("refused {r}"))
        )),
    }
}

/// A `cv` case: the bytes decode as one `cv` to their end and re-encode byte for byte ([F07 §7.1]).
fn check_cv(f: &Framed) -> Result<(), String> {
    let b = hex_of(f, "hex")?.ok_or("no hex block")?;
    length_is(f, "length", &b)?;
    let mut r = Reader::new(&b);
    let v = Cv::decode(&mut r).map_err(|e| e.to_string())?;
    if !r.is_empty() {
        return Err(format!("bytes after the cv at offset {}", r.offset()));
    }
    if v.encode() != b {
        return Err(format!("{v:?} re-encodes to other bytes"));
    }
    Ok(())
}

/// A commit case's ids: item 10 ([F07 §10.4]), C and `commit_id` (§3.1), C rebuilt from the `commit` block, and for
/// `message-input-hex` the stored message is `N` of it (INDEX.md §5.1 step 4).
fn check_commit(f: &Framed) -> Result<(), String> {
    let digest = f.line("changeset-digest").map(id32).transpose()?;
    match (f.block("digest-input"), digest) {
        (Some(block), Some(d)) => {
            let di = hex_block(block).map_err(|e| format!("digest-input: {e}"))?;
            if blake3_256(&di) != d {
                return Err("changeset-digest is not BLAKE3-256 of digest-input".into());
            }
            let n: u64 = f
                .line("entry-count")
                .ok_or("no entry-count line")?
                .parse()
                .map_err(|_| "entry-count is not a count")?;
            check_digest_parts(block, n, &d)?;
        }
        _ => return Err("a commit case without digest-input and changeset-digest".into()),
    }
    let commit_id = f.line("commit-id").map(id32).transpose()?;
    let block = f.block("commit");
    match (hex_of(f, "c")?, commit_id, block) {
        (Some(cb), Some(id), Some(block)) => {
            if blake3_256(&cb) != id {
                return Err("commit-id is not BLAKE3-256 of c".into());
            }
            let d = digest.ok_or("a commit block without changeset-digest")?;
            let items = items_of(block, d)?;
            let rebuilt = items.input();
            if rebuilt != cb {
                let at = rebuilt
                    .iter()
                    .zip(&cb)
                    .position(|(a, b)| a != b)
                    .unwrap_or(rebuilt.len().min(cb.len()));
                return Err(format!(
                    "C rebuilt from the commit block differs from c at byte {at} (lengths {} and {})",
                    rebuilt.len(),
                    cb.len()
                ));
            }
            if let Some(m) = hex_of(f, "message-input-hex")? {
                let n = normalise(&m).map_err(|r| format!("N refuses message-input-hex: {r:?}"))?;
                if n != items.message {
                    return Err(format!(
                        "N(message-input-hex) is {n:?}, not the commit block's message {:?}",
                        items.message
                    ));
                }
            }
            Ok(())
        }
        _ => Err("c, commit-id and the commit block come together".into()),
    }
}

/// The checks of one case.
fn check(c: &Case) -> Result<(), String> {
    let f = &c.framed;
    match what(f) {
        What::Commit => check_commit(f),
        What::Message => check_message(f),
        What::Cv => check_cv(f),
        What::Unit => {
            let b = hex_of(f, "hex")?.ok_or("no hex block")?;
            length_is(f, "length", &b)
        }
    }
}

/// Fixtures whose conclusion differs from the oracle's reading of the specification, reported as spec findings until the
/// ruling lands. The walk runs them as expected failures: one that passes, or names no fixture, fails the walk.
const KNOWN: &[&str] = &[];

/// Every canonical case: the 25 commit ids of INDEX.md §5, the 25 cases of `N` and `N_imp` of
/// §1, and every `cv` case are recomputed.
#[test]
fn canonical_ids() {
    let all = cases();
    let count = |w: What| all.iter().filter(|c| what(&c.framed) == w).count();
    let (commits, messages, cvs) = (count(What::Commit), count(What::Message), count(What::Cv));
    let with_ids = all
        .iter()
        .filter(|c| c.framed.line("commit-id").is_some())
        .count();
    assert!(
        commits == 25 && with_ids == 25 && messages == 25 && cvs > 0,
        "fixtures/canonical: {commits} commit cases ({with_ids} with a commit-id), {messages} message cases and {cvs} cv \
         cases; INDEX.md states 25 commit ids and 25 messages"
    );
    run_all("fixtures/canonical", &all, |c| c.name.clone(), KNOWN, check);
}
