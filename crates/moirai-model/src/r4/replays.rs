//! The shape of replay rows 1, 3 and 4 of [40 §8.3.4] on synthetic histories (`docs/m0/PLAN.md` §3.2 WP-76 and §6.2
//! R16: at M0 they run through the model's exact-evidence resolution with git renames as data).
//!
//! The histories are random: renames, copies that share a blob, deletions of two copies with one re-add (an identical-
//! blob group), edits, additions, and renames made on a side branch that a merge commit brings in (diffed against its
//! first parent), with a lane that forks at the root. The generator records the fate of every deleted path of every
//! first-parent commit — renamed exactly to y, or part of a group — independently of the model.
//!
//! - **Row 1**: for every first-parent commit c and every path x it deletes, a link at x observed at c's first parent
//!   is `moved-auto` to y by `git/r100` exactly when c renamed x to y, and is never `moved-auto` otherwise.
//! - **Row 4**: a dead path follows its chain of exact renames — through gate row G2 from the root commit, and through
//!   G4's integration window from the lane — to where the chain ends, and a chain through a group is never re-bound.
//! - **Row 3**: mentions of moved-away paths are found through aliases and `path_moves`.

use crate::r4::cascade::{FileNode, Params, Runtime, View, resolve_file};
use crate::r4::git::{Commit, Git, Head, Repo};
use crate::r4::mentions::{Via, mentions};
use crate::r4::path::Os;
use crate::r4::strings::State;
use crate::r4::tests::runner;
use crate::r4::text::oid;
use crate::r4::tree::{Fs, VolumeCaps};
use crate::r4::uid::FileStatus;
use crate::value::{Algo, MoveClass, PathMove, PathVal};
use proptest::prelude::*;
use std::collections::BTreeMap;

const ROOT: &str = "D:/repos/app";

fn bytes_of(blob: &str) -> Vec<u8> {
    format!("content of blob {blob}\nwith a second line\n").into_bytes()
}

fn node(path: &str, blob: &str, g: &str) -> FileNode {
    FileNode {
        n: 1,
        root: "project".into(),
        path: path.into(),
        oid: Some(oid(Algo::Sha1, &bytes_of(blob))),
        bytes: Some(bytes_of(blob).len() as u64),
        observed_git: Some(g.into()),
        observed_blob: Some(blob.into()),
        relink: None,
        aliases: Vec::new(),
        status: FileStatus::Present,
        artifact_kind: None,
        tombstone: false,
        obs_hlc: 0,
        conflict: None,
        path_claim: false,
    }
}

/// One step of a history.
#[derive(Clone, Debug)]
enum Step {
    /// A path renamed to a fresh name.
    Rename(usize),
    /// A fresh path added with an existing path's blob (a copy: the blob is now shared).
    Copy(usize),
    /// Two paths sharing a blob deleted and one fresh path added with it: an identical-blob group.
    Group(usize),
    /// A path's blob replaced.
    Edit(usize),
    /// A fresh path with a fresh blob.
    Add,
    /// A side branch renames a path, and a merge commit brings it in.
    SideRename(usize),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0usize..16).prop_map(Step::Rename),
        2 => (0usize..16).prop_map(Step::Copy),
        1 => (0usize..16).prop_map(Step::Group),
        2 => (0usize..16).prop_map(Step::Edit),
        1 => Just(Step::Add),
        2 => (0usize..16).prop_map(Step::SideRename),
    ]
}

/// A generated history: the repository, the first-parent line oldest first (c0 included), and per first-parent commit
/// the fate of each path it deletes (`Some(y)`: renamed exactly to y; `None`: part of a group).
struct History {
    repo: Repo,
    line: Vec<String>,
    fates: BTreeMap<String, BTreeMap<String, Option<String>>>,
}

fn history(steps: &[Step]) -> History {
    let mut tree: BTreeMap<String, String> = (0..4)
        .map(|i| (format!("src/f{i}.rs"), format!("b{i}")))
        .collect();
    let mut commits = vec![Commit {
        id: "c0".into(),
        parents: Vec::new(),
        committer_time: 1_000,
        author_time: 1_000,
        tree: tree.clone(),
    }];
    // The lane forks at the root and only adds a file.
    let mut lane_tree = tree.clone();
    lane_tree.insert("lane/only.rs".into(), "lane".into());
    commits.push(Commit {
        id: "lane".into(),
        parents: vec!["c0".into()],
        committer_time: 1_001,
        author_time: 1_001,
        tree: lane_tree,
    });
    let mut line = vec!["c0".to_string()];
    let mut fates: BTreeMap<String, BTreeMap<String, Option<String>>> = BTreeMap::new();
    let mut fresh = 100u32;
    let name = |fresh: &mut u32, dir: &str| {
        *fresh += 1;
        format!("{dir}/r{fresh}.rs")
    };
    for (k, s) in steps.iter().enumerate() {
        let id = format!("c{}", k + 1);
        let parent = line.last().unwrap().clone();
        let time = 2_000 + 10 * k as i64;
        let paths: Vec<String> = tree.keys().cloned().collect();
        let mut fate: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut parents = vec![parent.clone()];
        match s {
            Step::Rename(i) => {
                let from = paths[i % paths.len()].clone();
                let blob = tree.remove(&from).unwrap();
                let dir = if fresh.is_multiple_of(2) {
                    "lib"
                } else {
                    "src"
                };
                let to = name(&mut fresh, dir);
                tree.insert(to.clone(), blob);
                fate.insert(from, Some(to));
            }
            Step::Copy(i) => {
                let blob = tree[&paths[i % paths.len()]].clone();
                tree.insert(name(&mut fresh, "copies"), blob);
            }
            Step::Group(i) => {
                let mut by_blob: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (p, b) in &tree {
                    by_blob.entry(b.clone()).or_default().push(p.clone());
                }
                let shared: Vec<(String, Vec<String>)> = by_blob
                    .into_iter()
                    .filter(|(_, ps)| ps.len() >= 2)
                    .collect();
                if shared.is_empty() {
                    let blob = tree[&paths[i % paths.len()]].clone();
                    tree.insert(name(&mut fresh, "copies"), blob);
                } else {
                    let (blob, ps) = &shared[i % shared.len()];
                    for p in &ps[..2] {
                        tree.remove(p);
                        fate.insert(p.clone(), None);
                    }
                    tree.insert(name(&mut fresh, "grouped"), blob.clone());
                }
            }
            Step::Edit(i) => {
                fresh += 1;
                tree.insert(paths[i % paths.len()].clone(), format!("e{fresh}"));
            }
            Step::Add => {
                fresh += 1;
                tree.insert(format!("src/new{fresh}.rs"), format!("n{fresh}"));
            }
            Step::SideRename(i) => {
                let from = paths[i % paths.len()].clone();
                let blob = tree.remove(&from).unwrap();
                let to = name(&mut fresh, "side");
                tree.insert(to.clone(), blob);
                let side = format!("s{}", k + 1);
                commits.push(Commit {
                    id: side.clone(),
                    parents: vec![parent.clone()],
                    committer_time: time - 5,
                    author_time: time - 5,
                    tree: tree.clone(),
                });
                parents.push(side);
                fate.insert(from, Some(to));
            }
        }
        commits.push(Commit {
            id: id.clone(),
            parents,
            committer_time: time,
            author_time: time,
            tree: tree.clone(),
        });
        fates.insert(id.clone(), fate);
        line.push(id);
    }
    History {
        repo: Repo {
            algo: Algo::Sha1,
            commits: commits.into_iter().map(|c| (c.id.clone(), c)).collect(),
            refs: BTreeMap::new(),
            blobs: BTreeMap::new(),
        },
        line,
        fates,
    }
}

fn at_head(repo: &Repo, head: &str) -> (Fs, Git) {
    let mut fs = Fs::default();
    fs.ensure_tree(ROOT, "D", VolumeCaps::NTFS, Os::Windows);
    fs.checkout(ROOT, repo.tau(head), bytes_of, 5_000).unwrap();
    let mut git = Git::default();
    let mut r = repo.clone();
    r.refs.insert("refs/heads/main".into(), head.into());
    git.repos.insert("app".into(), r);
    git.heads.insert(
        ROOT.into(),
        ("app".into(), Head::Ref("refs/heads/main".into())),
    );
    (fs, git)
}

fn resolve_at(fs: &Fs, git: &Git, f: &FileNode, main: bool) -> crate::r4::cascade::FileResult {
    let view = View {
        files: vec![f.clone()],
        moves: BTreeMap::new(),
        anchor_conflicts: Vec::new(),
    };
    let p = Params {
        settle: true,
        writer: true,
        main,
        ..Params::default()
    };
    resolve_file(f, &view, fs, git, &Runtime::default(), ROOT, &p)
}

#[test]
fn row_1_every_exact_rename_is_moved_auto_and_nothing_else_is() {
    runner(96)
        .run(&proptest::collection::vec(step(), 1..12), |steps| {
            let h = history(&steps);
            for w in h.line.windows(2) {
                let (c1, c) = (&w[0], &w[1]);
                let (fs, git) = at_head(&h.repo, c);
                let before = h.repo.tau(c1);
                let after = h.repo.tau(c);
                for (x, blob) in before.iter().filter(|(x, _)| !after.contains_key(*x)) {
                    let r = resolve_at(&fs, &git, &node(x, blob, c1), true);
                    match h.fates[c].get(x) {
                        Some(Some(y)) => {
                            prop_assert_eq!(r.state, State::MovedAuto, "{} -> {} in {}", x, y, c);
                            prop_assert_eq!(r.at.as_deref(), Some(y.as_str()));
                            prop_assert_eq!(r.evidence.map(|e| e.0), Some(8), "git/r100");
                            prop_assert!(r.committed && r.fresh);
                        }
                        _ => prop_assert!(
                            r.state != State::MovedAuto,
                            "{} deleted in {} without an exact pair: {:?}",
                            x,
                            c,
                            r
                        ),
                    }
                }
            }
            Ok(())
        })
        .unwrap();
}

/// Where a path's chain of exact renames ends over the first-parent commits `window` (oldest first), by the recorded
/// fates: `Some(y)` after at least one exact step, `None` when the path takes no step or meets a group.
fn chain_end(h: &History, window: &[String], x: &str) -> Option<String> {
    let mut at = x.to_string();
    let mut stepped = false;
    for c in window {
        match h.fates[c].get(&at) {
            Some(Some(y)) => {
                at = y.clone();
                stepped = true;
            }
            Some(None) => return None,
            None => {}
        }
    }
    stepped.then_some(at)
}

#[test]
fn row_4_dead_paths_follow_their_unique_chains() {
    runner(96)
        .run(&proptest::collection::vec(step(), 2..14), |steps| {
            let h = history(&steps);
            let last = h.line.last().unwrap().clone();
            let (fs, git) = at_head(&h.repo, &last);
            let window = &h.line[1..];
            for (x, blob) in h.repo.tau("c0") {
                if h.repo.tau(&last).contains_key(x) {
                    continue;
                }
                // G2 from the root commit, and G4 from the lane, which forked at the root and never saw the renames.
                for g in ["c0", "lane"] {
                    let r = resolve_at(&fs, &git, &node(x, blob, g), false);
                    match chain_end(&h, window, x) {
                        Some(y) => {
                            prop_assert_eq!(
                                (r.state, r.at.as_deref()),
                                (State::MovedAuto, Some(y.as_str())),
                                "{} from {}: {:?}",
                                x,
                                g,
                                r
                            );
                            prop_assert!(r.fresh);
                        }
                        None => {
                            prop_assert!(r.state != State::MovedAuto, "{} from {}: {:?}", x, g, r)
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn row_4_after_a_patch_integration_the_chain_starts_at_p() {
    // A lane commit l1 (not an ancestor of trunk) observed the file at old.rs; trunk integrated the change as a patch
    // (t1) and later renamed the file (t2): G4's integration window from merge-base(l1, t2) finds the chain from p.
    let mut commits = BTreeMap::new();
    let mk = |id: &str, ps: &[&str], t: i64, tree: &[(&str, &str)]| Commit {
        id: id.into(),
        parents: ps.iter().map(|s| s.to_string()).collect(),
        committer_time: t,
        author_time: t,
        tree: tree
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    };
    for c in [
        mk("base", &[], 1, &[("old.rs", "v1")]),
        mk("l1", &["base"], 2, &[("old.rs", "v2")]),
        mk("t1", &["base"], 3, &[("old.rs", "v2")]),
        mk("t2", &["t1"], 4, &[("sub/new.rs", "v2")]),
    ] {
        commits.insert(c.id.clone(), c);
    }
    let repo = Repo {
        algo: Algo::Sha1,
        commits,
        refs: BTreeMap::new(),
        blobs: BTreeMap::new(),
    };
    let (fs, git) = at_head(&repo, "t2");
    let r = resolve_at(&fs, &git, &node("old.rs", "v2", "l1"), false);
    assert_eq!(
        (r.state, r.at.as_deref()),
        (State::MovedAuto, Some("sub/new.rs"))
    );
    assert!(
        r.fresh,
        "E6 over the integration window found a chain from p"
    );
    // The same link read in a tree whose history has only the alias's chain: moved differently on this line.
    let mut fa = node("elsewhere.rs", "v2", "l1");
    fa.aliases = vec!["old.rs".into()];
    let r = resolve_at(&fs, &git, &fa, false);
    assert_eq!((r.state, r.details[0].code), (State::MovedNeedsConfirm, 20));
}

#[test]
fn row_3_mentions_through_aliases_and_path_moves() {
    let mut f = node("docs/archive/PHASE-X-PLAN.md", "b", "c0");
    f.aliases = vec!["docs/PHASE-X-PLAN.md".into()];
    let mut g = node("crates/engine/src/sync/lock.rs", "b2", "c0");
    g.n = 2;
    let view = View {
        files: vec![f, g],
        moves: BTreeMap::from([(
            "project".to_string(),
            vec![PathMove {
                hlc: 7,
                class: MoveClass::Committed,
                from: PathVal {
                    root: "project".into(),
                    text: "crates/engine/src/".into(),
                },
                to: PathVal {
                    root: "project".into(),
                    text: "crates/engine/src/sync/".into(),
                },
                git: None,
            }],
        )]),
        anchor_conflicts: Vec::new(),
    };
    let body = "Plan: docs/PHASE-X-PLAN.md (see also crates/engine/src/lock.rs).\nUnrelated: docs/other.md.";
    let m = mentions(body, &view, "project");
    assert_eq!(m.len(), 2);
    assert_eq!(
        m.iter().map(|x| x.via).collect::<Vec<_>>(),
        [Via::Alias, Via::PathMove]
    );
    assert_eq!(m[1].now, "crates/engine/src/sync/lock.rs");
}
