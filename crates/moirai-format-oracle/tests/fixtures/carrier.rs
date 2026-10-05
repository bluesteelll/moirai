//! `fixtures/carrier` (WP-21): per case (`fixtures/carrier/INDEX.md` §2–§4) the commit object parses and re-encodes;
//! the tree rebuilt from the first parent's tree, `old/` and `new/` lists exactly the blobs of `tree.lst` and passes the
//! image check ([F14 §3]–§9); the classification is the case's `class` ([F14 §10.9]); and items 1–9, re-derived from
//! the carriers alone ([F14 §12.1]) with item 10 as the case states it, give the case's `c` and `commit-id`
//! ([F07 §3.1]); a native case verifies against `Moirai-Commit` with the exporter's bytes (§10.1–§10.5); a demoted case
//! states the id of its failed native reconstruction, which differs from `Moirai-Commit` unless a `Moirai-Ops` that is
//! not the entry count demoted it ([F14 §12.1]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use moirai_format_oracle::fixture::{Framed, hex_block, parse_framed};
use moirai_format_oracle::image::commit::{
    self as icommit, CarrierCtx, CommitObj, ParentInfo, Verdict,
};
use moirai_format_oracle::image::git;
use moirai_format_oracle::image::tree::check_tree;
use moirai_format_oracle::prim::{Algo, Oid, blake3_256, hex, unhex};

use super::common::{blobs, check_digest_parts, family, read, run_all, text};

/// One carrier case with the values the checks and the other cases use.
pub struct Case {
    /// The directory name.
    pub name: String,
    /// The case directory.
    pub dir: PathBuf,
    /// `case.txt`.
    pub framed: Framed,
    /// The destination's object format.
    pub algo: Algo,
    /// `git-commit`.
    pub git_commit: Oid,
    /// `git-tree`.
    pub git_tree: Oid,
    /// `git-parent`: (object id, case name), in order.
    pub parents: Vec<(Oid, String)>,
    /// `commit-id`: the id the store holds for this git commit.
    pub commit_id: [u8; 32],
    /// `trailer-commit`: its `Moirai-Commit`, for a native candidate.
    pub trailer: Option<[u8; 32]>,
    /// The `hlc` of its `commit` block.
    pub hlc: u64,
    /// `changeset-digest`.
    pub changeset_digest: [u8; 32],
    /// `entry-count`: the item-10 entries of its tree diff ([F07 §10.4]), which `Moirai-Ops` states.
    pub entry_count: u64,
    /// The schema version of its tree's marker (1 when the tree rebuild fails; the case's own check reports that).
    pub schema_version: u32,
}

fn oid(algo: Algo, s: &str) -> Result<Oid, String> {
    let b = unhex(s).ok_or_else(|| format!("{s:?} is not hex"))?;
    match (algo, b.len()) {
        (Algo::Sha1, 20) => Ok(Oid::Sha1(b.try_into().expect("20"))),
        (Algo::Sha256, 32) => Ok(Oid::Sha256(b.try_into().expect("32"))),
        _ => Err(format!("{s} is not an object id of {}", algo.name())),
    }
}

fn id32(s: &str) -> Result<[u8; 32], String> {
    unhex(s)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("{s:?} is not 64 hex digits"))
}

fn line<'a>(f: &'a Framed, k: &str) -> Result<&'a str, String> {
    f.line(k).ok_or_else(|| format!("no `{k}` line"))
}

impl Case {
    fn load(dir: PathBuf) -> Result<Case, String> {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        let recs = parse_framed(&text(&dir.join("case.txt"))).map_err(|e| e.to_string())?;
        let [framed]: [Framed; 1] = recs
            .try_into()
            .map_err(|v: Vec<Framed>| format!("{} records, not one", v.len()))?;
        if framed.line("case") != Some(name.as_str()) {
            return Err("the `case` line is not the directory name".into());
        }
        let dest: Vec<&str> = line(&framed, "destination")?.split(' ').collect();
        let algo = match dest.get(1) {
            Some(&"sha1") => Algo::Sha1,
            Some(&"sha256") => Algo::Sha256,
            _ => return Err("the destination names no object format".into()),
        };
        let mut parents = Vec::new();
        for p in framed.all("git-parent") {
            let (o, c) = p
                .split_once(' ')
                .ok_or_else(|| format!("git-parent {p:?} lacks its case"))?;
            parents.push((oid(algo, o)?, c.to_owned()));
        }
        let commit = framed.block("commit").ok_or("no `commit` block")?;
        let hlc = commit
            .iter()
            .find_map(|l| l.strip_prefix("hlc "))
            .and_then(|h| u64::from_str_radix(h, 16).ok())
            .ok_or("the `commit` block has no hex hlc")?;
        Ok(Case {
            git_commit: oid(algo, line(&framed, "git-commit")?)?,
            git_tree: oid(algo, line(&framed, "git-tree")?)?,
            commit_id: id32(line(&framed, "commit-id")?)?,
            trailer: framed.line("trailer-commit").map(id32).transpose()?,
            changeset_digest: id32(line(&framed, "changeset-digest")?)?,
            entry_count: line(&framed, "entry-count")?
                .parse()
                .map_err(|_| "entry-count is not a count")?,
            schema_version: 1,
            hlc,
            parents,
            algo,
            name,
            dir,
            framed,
        })
    }
}

/// Every carrier case, indexed by name and by git id.
pub struct Cases {
    /// The cases in name order.
    pub list: Vec<Case>,
}

/// A tree as (path, bytes) by path.
type Files = BTreeMap<String, Vec<u8>>;

impl Cases {
    /// Loads every case directory of `fixtures/carrier`, then the schema version of each tree's marker.
    pub fn load() -> Cases {
        let base = family("carrier");
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&base)
            .expect("list fixtures/carrier")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        let list = dirs
            .into_iter()
            .map(|d| {
                let n = d.display().to_string();
                Case::load(d).unwrap_or_else(|e| panic!("{n}: {e}"))
            })
            .collect();
        let mut cases = Cases { list };
        let versions: Vec<u32> = cases
            .list
            .iter()
            .map(|c| {
                cases
                    .files(&c.name)
                    .ok()
                    .and_then(|f| f.get(".moirai-image").cloned())
                    .and_then(|m| moirai_format_oracle::image::files::parse_marker(&m).ok())
                    .map_or(1, |m| m.schema_version as u32)
            })
            .collect();
        for (c, v) in cases.list.iter_mut().zip(versions) {
            c.schema_version = v;
        }
        cases
    }

    /// The case named `name`.
    pub fn get(&self, name: &str) -> Option<&Case> {
        self.list.iter().find(|c| c.name == name)
    }

    /// The case whose git commit is `o`.
    pub fn by_git(&self, o: &Oid) -> Option<&Case> {
        self.list.iter().find(|c| c.git_commit == *o)
    }

    /// A case whose root tree is `o`.
    pub fn by_tree(&self, o: &Oid) -> Option<&Case> {
        self.list.iter().find(|c| c.git_tree == *o)
    }

    /// The case's whole tree: its first parent's tree less `old/`, plus `new/` (`fixtures/carrier/INDEX.md` §2).
    pub fn files(&self, name: &str) -> Result<Files, String> {
        let c = self.get(name).ok_or_else(|| format!("no case {name}"))?;
        let mut files = match c.parents.first() {
            Some((_, p)) => self.files(p)?,
            None => Files::new(),
        };
        let old = c.dir.join("old");
        if old.exists() {
            for (p, b) in blobs(&old) {
                match files.remove(&p) {
                    Some(x) if x == b => {}
                    Some(_) => return Err(format!("old/{p} differs from the first parent's blob")),
                    None => return Err(format!("old/{p} is not in the first parent's tree")),
                }
            }
        }
        let new = c.dir.join("new");
        if new.exists() {
            for (p, b) in blobs(&new) {
                files.insert(p, b);
            }
        }
        Ok(files)
    }
}

/// One line of a `tree.lst` (`<mode> <type> <oid>` HT `<path>`, `fixtures/carrier/INDEX.md` §2).
struct Listed {
    /// `100644` (a blob) or `40000` (a tree), [F14 §3.2].
    mode: &'static str,
    /// The object id.
    oid: Oid,
    /// The path from the root.
    path: String,
}

fn listing(t: &str, algo: Algo) -> Result<Vec<Listed>, String> {
    let mut out = Vec::new();
    for l in t.lines() {
        let (meta, path) = l
            .split_once('\t')
            .ok_or_else(|| format!("tree.lst line {l:?} has no HT"))?;
        let (mode, o) = match meta.split(' ').collect::<Vec<_>>().as_slice() {
            ["100644", "blob", o] => ("100644", *o),
            ["40000", "tree", o] => ("40000", *o),
            _ => {
                return Err(format!(
                    "tree.lst line {l:?} is not a blob or a tree of [F14 §3.2]"
                ));
            }
        };
        out.push(Listed {
            mode,
            oid: oid(algo, o)?,
            path: path.to_owned(),
        });
    }
    Ok(out)
}

/// [F14 §3.2]: every blob id of the listing is the id of the rebuilt file, every tree's entries are in git's order,
/// every tree id is the id of the tree object built from its entries, and the root tree's id is `root`.
fn check_objects(listed: &[Listed], files: &Files, root: &Oid, algo: Algo) -> Result<(), String> {
    let blob_paths: Vec<&str> = listed
        .iter()
        .filter(|x| x.mode == "100644")
        .map(|x| x.path.as_str())
        .collect();
    let have: Vec<&str> = files.keys().map(String::as_str).collect();
    let mut sorted = blob_paths.clone();
    sorted.sort_unstable();
    if have != sorted {
        return Err(format!(
            "the rebuilt tree {have:?} is not tree.lst's blobs {sorted:?}"
        ));
    }
    for x in listed.iter().filter(|x| x.mode == "100644") {
        if git::object_id("blob", &files[&x.path], algo) != x.oid {
            return Err(format!(
                "{}: its bytes do not hash to its tree.lst blob id",
                x.path
            ));
        }
    }
    let parent_of = |p: &str| {
        p.rsplit_once('/')
            .map_or(String::new(), |(d, _)| d.to_owned())
    };
    let mut dirs: Vec<String> = listed
        .iter()
        .filter(|x| x.mode == "40000")
        .map(|x| x.path.clone())
        .collect();
    dirs.push(String::new());
    for d in &dirs {
        let entries: Vec<git::TreeEntry> = listed
            .iter()
            .filter(|x| parent_of(&x.path) == *d)
            .map(|x| git::TreeEntry {
                mode: x.mode,
                name: x.path.rsplit('/').next().unwrap_or(&x.path).to_owned(),
                oid: x.oid,
            })
            .collect();
        if entries.is_empty() || !git::in_git_order(&entries) {
            return Err(format!(
                "tree {d:?} is empty or not in git's entry order [F14 §3.2]"
            ));
        }
        let got = git::object_id("tree", &git::tree_object(&entries), algo);
        let want = if d.is_empty() {
            *root
        } else {
            listed
                .iter()
                .find(|x| x.path == *d)
                .expect("a listed tree")
                .oid
        };
        if got != want {
            return Err(format!("tree {d:?}: its entries do not hash to its id"));
        }
    }
    Ok(())
}

fn check(c: &Case, cases: &Cases) -> Result<(), String> {
    let f = &c.framed;
    // The commit object ([F14 §10.1]).
    let bytes = read(&c.dir.join("commit"));
    let obj: CommitObj = icommit::parse_commit_object(&bytes, c.algo).map_err(|e| e.to_string())?;
    if icommit::encode_commit_object(&obj) != bytes {
        return Err("the commit object does not re-encode byte for byte".into());
    }
    if obj.tree != c.git_tree {
        return Err("the commit's tree line is not git-tree".into());
    }
    let git_parents: Vec<Oid> = c.parents.iter().map(|p| p.0).collect();
    if obj.parents != git_parents {
        return Err("the commit's parent lines are not the git-parent lines".into());
    }
    // The objects ([F14 §3.2], §10.1): the commit's own id, and the tree rebuilt from the parent chain, whose blobs and
    // trees hash to tree.lst's ids; parent-tree.lst is the first parent's tree.lst; new/ and old/ are exactly the blobs
    // that differ between the two listings.
    let own = git::object_id("commit", &bytes, c.algo);
    if own != c.git_commit {
        return Err("the commit object does not hash to git-commit".into());
    }
    let files = cases.files(&c.name)?;
    let listed = listing(&text(&c.dir.join("tree.lst")), c.algo)?;
    check_objects(&listed, &files, &c.git_tree, c.algo)?;
    let parent_lst = c.dir.join("parent-tree.lst");
    let parent_listed = match c.parents.first() {
        Some((_, p)) => {
            let pc = cases
                .get(p)
                .ok_or_else(|| format!("git-parent names no case {p}"))?;
            let t = text(&parent_lst);
            if t != text(&pc.dir.join("tree.lst")) {
                return Err(format!("parent-tree.lst is not the tree.lst of {p}"));
            }
            listing(&t, c.algo)?
        }
        None if parent_lst.exists() => {
            return Err("parent-tree.lst of a commit without a parent".into());
        }
        None => Vec::new(),
    };
    let blobs_of = |l: &[Listed]| -> Vec<(String, Oid)> {
        l.iter()
            .filter(|x| x.mode == "100644")
            .map(|x| (x.path.clone(), x.oid))
            .collect()
    };
    let (now, before) = (blobs_of(&listed), blobs_of(&parent_listed));
    let mut want_new: Vec<String> = now
        .iter()
        .filter(|x| !before.contains(x))
        .map(|x| x.0.clone())
        .collect();
    let mut want_old: Vec<String> = before
        .iter()
        .filter(|x| !now.contains(x))
        .map(|x| x.0.clone())
        .collect();
    want_new.sort();
    want_old.sort();
    for (k, d, want) in [
        ("touched-new", "new", &want_new),
        ("touched-old", "old", &want_old),
    ] {
        let dir = c.dir.join(d);
        let got: Vec<String> = if dir.exists() {
            blobs(&dir).into_iter().map(|x| x.0).collect()
        } else {
            Vec::new()
        };
        if got != *want {
            return Err(format!(
                "{d}/ holds {got:?}, not the differing blobs {want:?}"
            ));
        }
        let n: usize = line(f, k)?
            .parse()
            .map_err(|_| format!("{k} is not a count"))?;
        if n != want.len() {
            return Err(format!("{k} is not the number of files under {d}/"));
        }
    }
    let blob_list: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    let tree = check_tree(&blob_list, c.algo).map_err(|e| format!("tree: {e}"))?;
    let exported = !HAND_WRITTEN.contains(&c.name.as_str())
        && matches!(line(f, "class")?, "native" | "checkpoint");
    if exported && let Some((p, _)) = tree.files.iter().find(|(_, x)| !x.canonical) {
        return Err(format!(
            "an exported tree holds {p}, which is not the exporter's encoding of its parse [F14 §15]"
        ));
    }
    // Item 10 as stated: the digest input hashes to changeset-digest, and its entries recompute it with the entry count
    // ([F07 §10.4]).
    let block = f.block("digest-input").ok_or("no digest-input")?;
    let di = hex_block(block).map_err(|e| e.to_string())?;
    if blake3_256(&di) != c.changeset_digest {
        return Err("BLAKE3-256 of digest-input is not changeset-digest".into());
    }
    let n: u64 = line(f, "entry-count")?
        .parse()
        .map_err(|_| "entry-count is not a count")?;
    check_digest_parts(block, n, &c.changeset_digest)?;
    // Items 1–9 from the carriers ([F14 §12.1]).
    let mut parents = Vec::new();
    for (_, p) in &c.parents {
        let pc = cases
            .get(p)
            .ok_or_else(|| format!("git-parent names no case {p}"))?;
        parents.push(ParentInfo {
            id: pc.commit_id,
            commit: pc.trailer,
            hlc: pc.hlc,
        });
    }
    let cx = CarrierCtx {
        parents,
        marker_schema_version: c.schema_version,
        algo: c.algo,
        own_oid: own,
        changeset_digest: c.changeset_digest,
        entry_count: n,
    };
    let (verdict, items) = icommit::import_items(&obj, &cx).map_err(|e| e.to_string())?;
    let class = line(f, "class")?;
    let verdict_ok = matches!(
        (&verdict, class),
        (Verdict::Native, "native")
            | (Verdict::Demoted(_), "demoted")
            | (Verdict::Checkpoint, "checkpoint")
            | (Verdict::Foreign, "foreign")
    );
    if !verdict_ok {
        return Err(format!("classified {verdict:?}, the case says {class}"));
    }
    let want_c = hex_block(f.block("c").ok_or("no `c` block")?).map_err(|e| e.to_string())?;
    let got_c = items.input();
    if got_c != want_c {
        let at = got_c
            .iter()
            .zip(&want_c)
            .position(|(a, b)| a != b)
            .unwrap_or(got_c.len().min(want_c.len()));
        return Err(format!(
            "the rebuilt C differs from `c` at byte {at} (lengths {} and {})",
            got_c.len(),
            want_c.len()
        ));
    }
    if blake3_256(&want_c) != c.commit_id || items.commit_id() != c.commit_id {
        return Err("commit-id is not BLAKE3-256 of `c`".into());
    }
    match &verdict {
        Verdict::Native => {
            if c.trailer != Some(c.commit_id) {
                return Err("a native case's commit-id is not its trailer-commit".into());
            }
            if exported {
                icommit::verify_native(&obj, &cx).map_err(|e| e.to_string())?;
            }
        }
        Verdict::Demoted(None) => {
            // [F14 §10.9]: the message part fails N step 1, so no native reconstruction exists to state.
            if f.line("native-commit-id").is_some() {
                return Err(
                    "native-commit-id for a commit demoted before any native reconstruction [F14 §10.9]"
                        .into(),
                );
            }
        }
        Verdict::Demoted(Some(native)) => {
            let want = id32(line(f, "native-commit-id")?)?;
            let got = native.commit_id();
            if got != want {
                return Err(format!(
                    "the failed native reconstruction {} is not native-commit-id",
                    hex(&got)
                ));
            }
            // [F14 §12.1]: a `Moirai-Ops` that is not the entry count demotes before any id is compared, so the
            // reconstruction may then equal `Moirai-Commit`; any other demotion is the id check failing (§12.6).
            let ops_demoted = matches!(
                icommit::classify(&obj.message),
                Ok(icommit::Class::Native(t)) if t.ops != Some(n)
            );
            if !ops_demoted && Some(want) == c.trailer {
                return Err(
                    "native-commit-id equals trailer-commit, yet Moirai-Ops is the entry count [F14 §12.1]"
                        .into(),
                );
            }
        }
        Verdict::Checkpoint | Verdict::Foreign => {}
    }
    for other in f.all("same-id") {
        let o = other.split(' ').next().unwrap_or(other);
        let oc = cases
            .get(o)
            .ok_or_else(|| format!("same-id names no case {o}"))?;
        if oc.commit_id != c.commit_id {
            return Err(format!("same-id {o} has another commit-id"));
        }
    }
    Ok(())
}

/// Native cases a person wrote (`fixtures/carrier/INDEX.md` §1 "Written by"): they verify by id alone; the exporter's
/// bytes (§10.1–§10.5) and canonical trees ([F14 §15]) are checked for every other native and checkpoint case.
const HAND_WRITTEN: &[&str] = &["defaults-explicit", "demoted-child"];

/// Fixtures whose conclusion differs from the oracle's reading of the specification, reported as spec findings until the
/// ruling lands. The walk runs them as expected failures: one that passes, or names no fixture, fails the walk.
const KNOWN: &[&str] = &[];

/// Every carrier case.
#[test]
fn carrier_cases() {
    let cases = Cases::load();
    assert_eq!(
        cases.list.len(),
        45,
        "fixtures/carrier/INDEX.md §5 lists 45 cases"
    );
    run_all(
        "fixtures/carrier",
        &cases.list,
        |c| c.name.clone(),
        KNOWN,
        |c| check(c, &cases),
    );
}
