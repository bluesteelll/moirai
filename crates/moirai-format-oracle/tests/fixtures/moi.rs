//! `fixtures/moi` (WP-21): every fixture is checked as its `.expect` says (`fixtures/moi/INDEX.md` §2, §5): a positive
//! file or tree parses by [F14] and encodes to the bytes its `canonical` line names ([F14 §15] rule 3); a negative
//! (every fixture under `neg*/`) is refused with `ImageParse` ([F14 §9.2]) for the rule its `refuse` line names
//! (`fixtures/moi/INDEX.md` §4.2).

use std::path::Path;

use moirai_format_oracle::fixture::{Framed, parse_framed};
use moirai_format_oracle::image::commit::{self as icommit, CarrierCtx, ParentInfo};
use moirai_format_oracle::image::git;
use moirai_format_oracle::image::schema::Schema;
use moirai_format_oracle::image::tree::{Tree, check_table_file, check_tree};
use moirai_format_oracle::image::{Checked, Parsed, Place, check_file, node};
use moirai_format_oracle::prim::{Algo, Oid, Result as DResult, Rule, hex};

use super::carrier::Cases;
use super::common::{blobs, family, read, rel, run_all, text, walk};

/// One `.expect` record with the directives the checks use.
struct Expect {
    /// The `.expect` path under `fixtures/moi/`.
    name: String,
    /// `fixture`.
    fixture: String,
    /// `file`.
    file: String,
    /// `context` (absent for the side-ref files).
    context: String,
    /// `canonical` (positives).
    canonical: Option<String>,
    /// `refuse` (negatives).
    refuse: Option<String>,
}

fn load(p: &Path, base: &Path) -> Result<Expect, String> {
    let recs = parse_framed(&text(p)).map_err(|e| e.to_string())?;
    let [r]: [Framed; 1] = recs
        .try_into()
        .map_err(|v: Vec<Framed>| format!("{} records, not one", v.len()))?;
    let need = |k: &str| {
        r.line(k)
            .map(str::to_owned)
            .ok_or_else(|| format!("no `{k}` directive"))
    };
    Ok(Expect {
        name: rel(p, base),
        fixture: need("fixture")?,
        file: need("file")?,
        context: r.line("context").unwrap_or("").to_owned(),
        canonical: r.line("canonical").map(str::to_owned),
        refuse: r.line("refuse").map(str::to_owned),
    })
}

/// Every `.expect` under `fixtures/moi`.
fn expects() -> Vec<Expect> {
    let base = family("moi");
    walk(&base)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "expect"))
        .map(|p| load(&p, &base).unwrap_or_else(|e| panic!("{}: {e}", rel(&p, &base))))
        .collect()
}

/// The object format a `context` line names (`destination object format <f>`; a carrier destination A is sha1).
fn algo_of(context: &str) -> Algo {
    if context.ends_with("sha256") {
        Algo::Sha256
    } else {
        Algo::Sha1
    }
}

/// The parse's variant must be the one `file` names.
fn kind_ok(file: &str, p: &Parsed) -> bool {
    matches!(
        (file, p),
        ("node", Parsed::Node(_))
            | ("schema", Parsed::Schema(_))
            | ("query", Parsed::Query(_))
            | ("marker", Parsed::Marker(_))
            | ("refs", Parsed::Refs(..))
            | ("meta", Parsed::Meta(_))
            | ("aliases", Parsed::Aliases(_))
            | ("ops", Parsed::Ops(_))
    )
}

/// The exporter's encoding of a parse; a live node file without `created:` or `updated:` ([F14 §9.1] item 8) first
/// gets them from its twin, which stands for the history the store fills them from.
fn with_history(c: &Checked, twin: &[u8]) -> Vec<u8> {
    let Parsed::Node(n) = &c.parsed else {
        return c.encoded.clone();
    };
    if n.tomb || (n.created.is_some() && n.updated.is_some()) {
        return c.encoded.clone();
    }
    let core = Schema::core();
    let Ok(Checked {
        parsed: Parsed::Node(t),
        ..
    }) = check_file(twin, Place::default(), &core)
    else {
        return c.encoded.clone();
    };
    let mut filled = (**n).clone();
    filled.created = filled.created.or_else(|| t.created.clone());
    filled.updated = filled.updated.or_else(|| t.updated.clone());
    node::encode(&filled, &core)
}

/// The canonical encoding check of a single file ([F14 §15] rule 3): `self`, another fixture's bytes, or `none`.
fn canonical_ok(e: &Expect, c: &Checked) -> Result<(), String> {
    match e.canonical.as_deref() {
        Some("self") if c.canonical => Ok(()),
        Some("self") => {
            Err("parsed, but its bytes are not the exporter's encoding of the parse".into())
        }
        Some("none") => Ok(()),
        Some(other) => {
            let twin = read(&family("moi").join(other));
            if with_history(c, &twin) == twin {
                Ok(())
            } else {
                Err(format!(
                    "its encoding is not the bytes of its canonical twin {other}"
                ))
            }
        }
        None => Err("a positive without a `canonical` line".into()),
    }
}

/// What a positive parse yields.
enum Outcome {
    /// A single file's check.
    File(Checked),
    /// A tree's check.
    Tree(Tree),
    /// A commit's import (the commit negatives only).
    Commit,
}

/// Runs the check the `.expect` names. The outer error is a harness failure (a context the fixtures do not give); the
/// inner result is the oracle's verdict.
fn parse(e: &Expect, cases: &Cases) -> Result<DResult<Outcome>, String> {
    let moi = family("moi");
    let path = moi.join(&e.fixture);
    Ok(match e.file.as_str() {
        "tree" => check_tree(&blobs(&path), algo_of(&e.context)).map(Outcome::Tree),
        "commit" => {
            let b = read(&path);
            match icommit::parse_commit_object(&b, Algo::Sha1) {
                Err(x) => Err(x),
                Ok(obj) => {
                    let own = git::object_id("commit", &b, Algo::Sha1);
                    let cx = context_of(&obj, own, cases)?;
                    icommit::import_items(&obj, &cx).map(|_| Outcome::Commit)
                }
            }
        }
        // A file named for its table also passes the table's rules ([F14 §3.1], §7.1); a negative's descriptive name
        // tells no table.
        "schema" => match e.fixture.rsplit('/').next().unwrap_or("") {
            t @ ("kinds.moi" | "fields.moi" | "edges.moi") => {
                check_table_file(&format!("schema/{t}"), &read(&path), &Schema::core())
            }
            _ => check_file(&read(&path), Place::default(), &Schema::core()),
        }
        .map(Outcome::File),
        _ => check_file(&read(&path), Place::of_path(&e.fixture), &Schema::core()).and_then(|c| {
            match &c.parsed {
                Parsed::Marker(m) if m.object_format != algo_of(&e.context).name() => {
                    moirai_format_oracle::image::text::parse_err(
                        Rule::Marker,
                        0,
                        "the marker's object format is not the destination's [F14 §4]",
                    )
                }
                _ => Ok(Outcome::File(c)),
            }
        }),
    })
}

/// The carrier context of a commit negative ([F14 §12.6]): its parents and tree are those of carrier cases of
/// destination A (found by git id), whose ids and `hlc` values the cases give; `own` is its object id. A third parent
/// needs no context ([F07 §12.3] refuses it first).
fn context_of(obj: &icommit::CommitObj, own: Oid, cases: &Cases) -> Result<CarrierCtx, String> {
    let mut parents = Vec::new();
    for p in &obj.parents {
        match cases.by_git(p) {
            Some(c) => parents.push(ParentInfo {
                id: c.commit_id,
                commit: c.trailer,
                hlc: c.hlc,
            }),
            None if obj.parents.len() > 2 => {}
            None => return Err(format!("parent {} names no carrier case", hex(p.digest()))),
        }
    }
    let tree_case = cases
        .by_tree(&obj.tree)
        .ok_or_else(|| format!("tree {} names no carrier case", hex(obj.tree.digest())))?;
    Ok(CarrierCtx {
        parents,
        marker_schema_version: tree_case.schema_version,
        algo: Algo::Sha1,
        own_oid: own,
        changeset_digest: tree_case.changeset_digest,
        entry_count: tree_case.entry_count,
    })
}

fn check(e: &Expect, cases: &Cases) -> Result<(), String> {
    let negative = e.fixture.starts_with("neg");
    match (&e.refuse, &e.canonical, negative) {
        (Some(r), None, true) if r.starts_with("ImageParse ") => {}
        (None, Some(_), false) => {}
        _ => {
            return Err(format!(
                "the name (negative: {negative}) disagrees with its `refuse`/`canonical` lines"
            ));
        }
    }
    match (parse(e, cases)?, negative) {
        // A negative is refused for the rule its `refuse ImageParse <rule>` line names (`fixtures/moi/INDEX.md` §4.2).
        (Err(x), true) => {
            let want = e
                .refuse
                .as_deref()
                .and_then(|r| r.strip_prefix("ImageParse "));
            if x.rule.map(Rule::id) == want {
                Ok(())
            } else {
                Err(format!(
                    "refused for rule {}, not {} as its name says: {x}",
                    x.rule.map_or("(none)", Rule::id),
                    want.unwrap_or("?")
                ))
            }
        }
        (Ok(_), true) => Err(format!(
            "parsed, but it is a negative ({})",
            e.refuse.as_deref().unwrap_or("")
        )),
        (Err(x), false) => Err(x.to_string()),
        (Ok(Outcome::Tree(t)), false) => {
            if e.canonical.as_deref() == Some("self") && !t.canonical() {
                return Err("a tree file is not the exporter's encoding of its parse".into());
            }
            Ok(())
        }
        (Ok(Outcome::Commit), false) => Err("a positive commit fixture has no check".into()),
        (Ok(Outcome::File(c)), false) => {
            if !kind_ok(&e.file, &c.parsed) {
                return Err(format!("parsed as another file than {}", e.file));
            }
            canonical_ok(e, &c)
        }
    }
}

/// Fixtures whose conclusion differs from the oracle's reading of the specification (reported as spec findings; each
/// has an ignored test of its own until the ruling lands).
const KNOWN: &[&str] = &[];

/// Every `.expect`'s fixture, checked as it says, but the known mismatches.
#[test]
fn moi_fixtures() {
    let cases = Cases::load();
    run_all(
        "fixtures/moi",
        &expects(),
        |e| e.name.clone(),
        KNOWN,
        |e| check(e, &cases),
    );
}

/// Every fixture has its `.expect`, and every `.expect` names an existing fixture.
#[test]
fn moi_expect_files_are_complete() {
    let base = family("moi");
    let ex = expects();
    let named: Vec<&str> = ex.iter().map(|e| e.fixture.as_str()).collect();
    let mut missing = Vec::new();
    for e in &ex {
        if !base.join(&e.fixture).exists() {
            missing.push(format!(
                "{}: names {}, which does not exist",
                e.name, e.fixture
            ));
        }
        let stem = e.name.strip_suffix(".expect").unwrap_or(&e.name);
        if !e.fixture.starts_with(stem) {
            missing.push(format!("{}: does not sit beside {}", e.name, e.fixture));
        }
    }
    for p in walk(&base) {
        let r = rel(&p, &base);
        if r == "INDEX.md" || r.ends_with(".expect") {
            continue;
        }
        let covered = named
            .iter()
            .any(|n| r == *n || (n.ends_with('/') && r.starts_with(n)));
        if !covered {
            missing.push(format!("{r}: no .expect names it"));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
