//! Git history as abstract data ([API §6.6]; [60 §4.2] row "File links"; `docs/m0/PLAN.md` §6.2 R16): repositories of
//! commits with parents, committer times and `path → blob id` maps, refs, and one HEAD per simulated tree; ancestry by
//! full ancestor sets, merge bases as maximal common ancestors, the first-parent windows of E6, per-commit changes and
//! exact pairs, the inexact pairs where blob contents are known, rename chains, and `committed` directory moves
//! ([F20 §5.11], §5.16). Rename matching uses git's blob ids, never moirai `oid`s.
//!
//! A tree entry has no mode in the abstract history, so every entry is of class `file`; a blob whose bytes the
//! history does not carry makes an inexact-pair test `Unavailable(git)`.

use crate::r4::text::{Ratio, containments, git_pair_score, tiny_content};
use crate::value::Algo;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// `E6_MAX_COMMITS` ([F20 §5.11.2]).
pub const E6_MAX_COMMITS: usize = 2000;
/// `E6_SLACK_MS` ([F20 §5.11.2]).
pub const E6_SLACK_MS: i128 = 86_400_000;

/// One commit ([API §6.6] `commits`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// The git id (hex).
    pub id: String,
    /// The parents, first parent first.
    pub parents: Vec<String>,
    /// Committer time, whole seconds.
    pub committer_time: i64,
    /// Author time, whole seconds.
    pub author_time: i64,
    /// The committed tree: path → blob id.
    pub tree: BTreeMap<String, String>,
}

/// One repository ([API §6.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    /// The object format.
    pub algo: Algo,
    /// Commits by id; a commit is added once and never changes.
    pub commits: BTreeMap<String, Commit>,
    /// `refs/heads/<name>` → commit id.
    pub refs: BTreeMap<String, String>,
    /// Blob contents the history carries, by blob id (the inexact pairs of [F20 §5.11.4] read them).
    pub blobs: BTreeMap<String, Vec<u8>>,
}

/// A tree's HEAD ([API §6.6] `heads`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// Symbolic: `refs/heads/<name>`.
    Ref(String),
    /// Detached at a commit.
    Detached(String),
}

/// The abstract git histories: repositories by name and the HEAD of every tree bound to one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Git {
    /// Repositories by name.
    pub repos: BTreeMap<String, Repo>,
    /// Tree root → (repository, HEAD).
    pub heads: BTreeMap<String, (String, Head)>,
}

impl Git {
    /// The repository and HEAD of a tree, when the tree is a git worktree.
    pub fn of_tree(&self, root: &str) -> Option<(&Repo, &Head)> {
        let (name, head) = self.heads.get(root)?;
        Some((self.repos.get(name)?, head))
    }
}

/// The class of a pair or a chain result ([F20 §1.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// `exact`.
    Exact,
    /// `strong`.
    Strong,
    /// `weak`.
    Weak,
}

/// The result of a rename chain over a window ([F20 §5.11.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chain {
    /// The chain took at least one step and ended at a path, with its class and the lowest pair score (a git
    /// similarity index; 100 for exact steps).
    Path {
        /// The final path.
        path: String,
        /// The class.
        class: Class,
        /// The lowest pair score of the chain.
        score: u32,
    },
    /// The start path was never deleted inside the window.
    NoStep,
    /// An ambiguous identical-blob group: its candidates.
    Ambiguous(Vec<String>),
    /// A split: the pieces.
    Split(Vec<String>),
    /// A merge into a host: the host and the old-in-new containment.
    Merged(String, Ratio),
    /// The path was deleted in this commit.
    Deleted(String),
    /// A blob the test needed is not in the history: `Unavailable(git)`.
    Unavailable,
}

/// The per-commit changes against the first parent ([F20 §5.11.1]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// In the first parent only: path → blob.
    pub deleted: BTreeMap<String, String>,
    /// In the commit only.
    pub added: BTreeMap<String, String>,
    /// In both with different blobs: path → (old, new).
    pub modified: BTreeMap<String, (String, String)>,
}

/// The outcome of the inexact tests for a deleted path ([F20 §5.11.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Inexact {
    /// The chain ends with this result (a split or a merge).
    End(Chain),
    /// A `strong` or `weak` pair continues the chain: the path, the class and git's similarity index.
    Step(String, Class, u32),
}

/// One exact-pair outcome for a deleted path ([F20 §5.11.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pair {
    /// |D| = 1 and |A| = 1: the exact pair.
    Exact(String),
    /// An ambiguous group: the candidates.
    Group(Vec<String>),
    /// No exact pair.
    None,
}

static EMPTY: BTreeMap<String, String> = BTreeMap::new();

impl Repo {
    /// The commit a HEAD names, when it can be read.
    pub fn head_commit(&self, h: &Head) -> Option<&str> {
        match h {
            Head::Ref(r) => self.refs.get(r).map(String::as_str),
            Head::Detached(c) => self.commits.get_key_value(c).map(|(k, _)| k.as_str()),
        }
    }

    /// τ(c): the committed tree of c, empty for an unknown commit.
    pub fn tau(&self, c: &str) -> &BTreeMap<String, String> {
        self.commits.get(c).map_or(&EMPTY, |x| &x.tree)
    }

    /// Whether the local object store holds a commit.
    pub fn has(&self, c: &str) -> bool {
        self.commits.contains_key(c)
    }

    /// The ancestors of c, c included, over the parents the store holds.
    // spec: [60 §4.2] "ahead/behind" and "LCA": full ancestor sets
    pub fn ancestors(&self, c: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut stack = vec![c.to_string()];
        while let Some(x) = stack.pop() {
            if !self.commits.contains_key(&x) || !out.insert(x.clone()) {
                continue;
            }
            stack.extend(self.commits[&x].parents.iter().cloned());
        }
        out
    }

    /// `a` is an ancestor of, or equal to, `b`.
    pub fn is_ancestor(&self, a: &str, b: &str) -> bool {
        self.has(a) && self.ancestors(b).contains(a)
    }

    /// The merge bases of a and b: the common ancestors that are not ancestors of another common ancestor.
    pub fn merge_bases(&self, a: &str, b: &str) -> BTreeSet<String> {
        let common: BTreeSet<String> = self
            .ancestors(a)
            .intersection(&self.ancestors(b))
            .cloned()
            .collect();
        common
            .iter()
            .filter(|x| {
                !common
                    .iter()
                    .any(|y| y != *x && self.ancestors(y).contains(*x))
            })
            .cloned()
            .collect()
    }

    /// The changes of commit c against its first parent (the empty tree for a root commit); a merge is diffed against
    /// its first parent only.
    // spec: [F20 §5.11.1] changes
    pub fn changes(&self, c: &str) -> Changes {
        let cur = self.tau(c);
        let first = self
            .commits
            .get(c)
            .and_then(|x| x.parents.first())
            .map_or(&EMPTY, |p| self.tau(p));
        let mut ch = Changes::default();
        for (p, b) in first {
            match cur.get(p) {
                None => {
                    ch.deleted.insert(p.clone(), b.clone());
                }
                Some(nb) if nb != b => {
                    ch.modified.insert(p.clone(), (b.clone(), nb.clone()));
                }
                _ => {}
            }
        }
        for (p, b) in cur {
            if !first.contains_key(p) {
                ch.added.insert(p.clone(), b.clone());
            }
        }
        ch
    }

    /// The exact-pair outcome of a deleted path x in a commit's changes ([F20 §5.11.1] "Exact pairs").
    // spec: [F20 §5.11.1] exact pairs
    pub fn exact_pair(ch: &Changes, x: &str) -> Pair {
        let Some(blob) = ch.deleted.get(x) else {
            return Pair::None;
        };
        let d = ch.deleted.values().filter(|b| *b == blob).count();
        let a: Vec<String> = ch
            .added
            .iter()
            .filter(|(_, b)| *b == blob)
            .map(|(p, _)| p.clone())
            .collect();
        match (d, a.len()) {
            (_, 0) => Pair::None,
            (1, 1) => Pair::Exact(a[0].clone()),
            _ => Pair::Group(a),
        }
    }

    /// The first-parent window from H, oldest first ([F20 §5.11.2]): the walk stops before the first commit that
    /// `stop` holds for, after a root commit, or after `E6_MAX_COMMITS` commits.
    // spec: [F20 §5.11.2] windows
    pub fn window(&self, head: &str, stop: impl Fn(&Commit) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = Some(head.to_string());
        while let Some(c) = cur {
            if out.len() >= E6_MAX_COMMITS {
                break;
            }
            let Some(x) = self.commits.get(&c) else { break };
            if stop(x) {
                break;
            }
            out.push(c.clone());
            cur = x.parents.first().cloned();
        }
        out.reverse();
        out
    }

    /// The window g..H of gate row G2, and of G4 when g is in the local object store ([F20 §5.11.2]: the walk stops at
    /// g or an ancestor of g).
    pub fn window_since(&self, g: &str, head: &str) -> Vec<String> {
        Hist::new(self).window_since(g, head)
    }

    /// The time-bounded window of G4 when g is not in the local object store ([F20 §5.11.2]): the walk stops at the
    /// first commit whose committer time t has `t × 10^9 < hlc_ns(h_obs) − E6_SLACK_MS × 10^6 − SKEW`.
    pub fn window_timed(&self, head: &str, h_obs: u64, skew_ns: i128) -> Vec<String> {
        let obs_ns = i128::from(h_obs >> 16) * 1_000_000;
        let bound = obs_ns - E6_SLACK_MS * 1_000_000 - skew_ns;
        self.window(head, |c| {
            i128::from(c.committer_time) * 1_000_000_000 < bound
        })
    }

    /// The rename chain from `x0` over a window, oldest commit first ([F20 §5.11.3]).
    pub fn chain(&self, window: &[String], x0: &str) -> Chain {
        Hist::new(self).chain(window, x0)
    }

    /// Whether one commit of the window renames p to q exactly (the copy rule's line 1, [F20 §5.9]).
    // spec: [F20 §5.9] copy rule line 1
    pub fn renamed_in_one_commit(&self, window: &[String], p: &str, q: &str) -> bool {
        Hist::new(self).renamed_in_one_commit(window, p, q)
    }

    /// The `committed` directory moves a window shows ([F20 §5.16]).
    pub fn committed_moves(&self, window: &[String], head: &str) -> Vec<(String, String, String)> {
        Hist::new(self).committed_moves(window, head)
    }
}

/// The reads of one repository within one command, memoised (the per-case RAM and time budget of [60 §4.2]): each
/// commit's changes against its first parent and each commit's ancestor set are computed once, however many
/// resolutions of a settle walk them. A repository does not change within a command, so every memoised value equals
/// the definition's.
#[derive(Debug)]
pub struct Hist<'a> {
    /// The repository.
    pub repo: &'a Repo,
    changes: RefCell<BTreeMap<String, Rc<Changes>>>,
    ancestors: RefCell<BTreeMap<String, Rc<BTreeSet<String>>>>,
}

impl<'a> Hist<'a> {
    /// Empty memos over a repository.
    pub fn new(repo: &'a Repo) -> Hist<'a> {
        Hist {
            repo,
            changes: RefCell::new(BTreeMap::new()),
            ancestors: RefCell::new(BTreeMap::new()),
        }
    }

    /// [`Repo::changes`], memoised.
    pub fn changes(&self, c: &str) -> Rc<Changes> {
        if let Some(ch) = self.changes.borrow().get(c) {
            return Rc::clone(ch);
        }
        let ch = Rc::new(self.repo.changes(c));
        self.changes
            .borrow_mut()
            .insert(c.to_string(), Rc::clone(&ch));
        ch
    }

    /// [`Repo::ancestors`], memoised.
    pub fn ancestors(&self, c: &str) -> Rc<BTreeSet<String>> {
        if let Some(a) = self.ancestors.borrow().get(c) {
            return Rc::clone(a);
        }
        let a = Rc::new(self.repo.ancestors(c));
        self.ancestors
            .borrow_mut()
            .insert(c.to_string(), Rc::clone(&a));
        a
    }

    /// `a` is an ancestor of, or equal to, `b`.
    pub fn is_ancestor(&self, a: &str, b: &str) -> bool {
        self.repo.has(a) && self.ancestors(b).contains(a)
    }

    /// The window g..H of gate row G2, and of G4 when g is in the local object store ([F20 §5.11.2]: the walk stops at
    /// g or an ancestor of g).
    // spec: [F20 §5.11.2] windows
    pub fn window_since(&self, g: &str, head: &str) -> Vec<String> {
        let anc = self.ancestors(g);
        self.repo.window(head, |c| anc.contains(&c.id))
    }

    /// The inexact-pair outcome of a deleted path without an exact pair ([F20 §5.11.4]), the table's rows in order; the
    /// containments are exact, or the estimates when a side exceeds `EXACT_LIMIT` ([`containments`]).
    // spec: [F20 §5.11.4] inexact pairs
    fn inexact(&self, ch: &Changes, x: &str) -> Result<Option<Inexact>, ()> {
        let blob_x = &ch.deleted[x];
        let old = self.repo.blobs.get(blob_x).ok_or(())?;
        if tiny_content(old) {
            return Ok(None);
        }
        // Added candidates: added entries in no exact pair or ambiguous group of c.
        let paired: BTreeSet<&String> = ch
            .added
            .iter()
            .filter(|(_, b)| ch.deleted.values().any(|d| d == *b))
            .map(|(p, _)| p)
            .collect();
        let added: Vec<(&String, &String)> = ch
            .added
            .iter()
            .filter(|(p, _)| !paired.contains(p))
            .collect();
        let mut scored = Vec::new();
        for (p, b) in &added {
            let new = self.repo.blobs.get(*b).ok_or(())?;
            let gs = git_pair_score(old, new);
            let (oin, nio) = containments(old, new);
            scored.push(((*p).clone(), gs, oin, nio));
        }
        // 2: split.
        let pieces: Vec<&(String, u32, Ratio, Ratio)> =
            scored.iter().filter(|s| s.3 >= Ratio::new(4, 5)).collect();
        if scored.len() >= 2 && pieces.len() >= 2 {
            let sum = pieces.iter().fold(Ratio::int(0), |acc, s| acc + s.2);
            if sum >= Ratio::new(3, 5) {
                return Ok(Some(Inexact::End(Chain::Split(
                    pieces.iter().map(|s| s.0.clone()).collect(),
                ))));
            }
        }
        // 3: exactly one added candidate at gs ≥ 90.
        let strong: Vec<&(String, u32, Ratio, Ratio)> =
            scored.iter().filter(|s| s.1 >= 90).collect();
        if strong.len() == 1 {
            return Ok(Some(Inexact::Step(
                strong[0].0.clone(),
                Class::Strong,
                strong[0].1,
            )));
        }
        let mut by_score: Vec<&(String, u32, Ratio, Ratio)> = scored.iter().collect();
        by_score.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        // 4: the best at gs ≥ 50, 20 above the next, same dirname or basename as x.
        if let Some(best) = by_score.first() {
            let next = by_score.get(1).map_or(0, |s| s.1);
            let dir = |p: &str| p.rfind('/').map_or("", |i| &p[..i]).to_string();
            let base = |p: &str| p.rfind('/').map_or(p, |i| &p[i + 1..]).to_string();
            if best.1 >= 50
                && best.1 >= next + 20
                && (dir(&best.0) == dir(x) || base(&best.0) == base(x))
            {
                return Ok(Some(Inexact::Step(best.0.clone(), Class::Strong, best.1)));
            }
        }
        // 5: merged into a host or added candidate.
        let mut hosts: Vec<(String, Ratio, Ratio)> = Vec::new();
        for (p, (_, nb)) in &ch.modified {
            let new = self.repo.blobs.get(nb).ok_or(())?;
            let (oin, nio) = containments(old, new);
            hosts.push((p.clone(), oin, nio));
        }
        for s in &scored {
            hosts.push((s.0.clone(), s.2, s.3));
        }
        hosts.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(h) = hosts
            .iter()
            .find(|h| h.1 >= Ratio::new(4, 5) && h.2 < Ratio::new(1, 2))
        {
            return Ok(Some(Inexact::End(Chain::Merged(h.0.clone(), h.1))));
        }
        // 6: weak.
        if let Some(best) = by_score.first()
            && best.1 >= 20
            && best.1 < 50
            && best.2.max(best.3) >= Ratio::new(4, 5)
        {
            return Ok(Some(Inexact::Step(best.0.clone(), Class::Weak, best.1)));
        }
        Ok(None)
    }

    /// The rename chain from `x0` over a window, oldest commit first ([F20 §5.11.3]).
    // spec: [F20 §5.11.3] chains
    pub fn chain(&self, window: &[String], x0: &str) -> Chain {
        let mut x = x0.to_string();
        let mut class = Class::Exact;
        let mut score = 100u32;
        let mut stepped = false;
        for c in window {
            let ch = self.changes(c);
            if !ch.deleted.contains_key(&x) {
                continue;
            }
            match Repo::exact_pair(&ch, &x) {
                Pair::Exact(y) => {
                    x = y;
                    stepped = true;
                }
                Pair::Group(cands) => return Chain::Ambiguous(cands),
                Pair::None => match self.inexact(&ch, &x) {
                    Err(()) => return Chain::Unavailable,
                    Ok(None) => return Chain::Deleted(c.clone()),
                    Ok(Some(Inexact::End(end))) => return end,
                    Ok(Some(Inexact::Step(y, cls, s))) => {
                        x = y;
                        class = class.max(cls);
                        score = score.min(s);
                        stepped = true;
                    }
                },
            }
        }
        if stepped {
            Chain::Path {
                path: x,
                class,
                score,
            }
        } else {
            Chain::NoStep
        }
    }

    /// Whether one commit of the window renames p to q exactly (the copy rule's line 1, [F20 §5.9]).
    // spec: [F20 §5.9] copy rule line 1
    pub fn renamed_in_one_commit(&self, window: &[String], p: &str, q: &str) -> bool {
        window
            .iter()
            .any(|c| Repo::exact_pair(&self.changes(c), p) == Pair::Exact(q.to_string()))
    }

    /// The `committed` directory moves a window shows ([F20 §5.16]): (from/, to/, commit) where one commit c renames
    /// every entry under `from/` of τ(c1) exactly to `to/ ‖ rest`, and no entry of τ(H) starts with `from/`; a move
    /// implied by a shorter one of the same commit is not listed.
    // spec: [F20 §5.16] committed
    pub fn committed_moves(&self, window: &[String], head: &str) -> Vec<(String, String, String)> {
        let tau_h = self.repo.tau(head);
        let mut out = Vec::new();
        for c in window {
            let ch = self.changes(c);
            let first = self
                .repo
                .commits
                .get(c)
                .and_then(|x| x.parents.first())
                .map_or(&EMPTY, |p| self.repo.tau(p));
            let mut cands: BTreeSet<(String, String)> = BTreeSet::new();
            for x in ch.deleted.keys() {
                if let Pair::Exact(y) = Repo::exact_pair(&ch, x) {
                    let xs: Vec<&str> = x.split('/').collect();
                    let ys: Vec<&str> = y.split('/').collect();
                    let mut k = 1;
                    while k < xs.len() && k < ys.len() && xs[xs.len() - k..] == ys[ys.len() - k..] {
                        let from = format!("{}/", xs[..xs.len() - k].join("/"));
                        let to = format!("{}/", ys[..ys.len() - k].join("/"));
                        if from != to {
                            cands.insert((from, to));
                        }
                        k += 1;
                    }
                }
            }
            let valid: Vec<(String, String)> = cands
                .into_iter()
                .filter(|(from, to)| {
                    let under: Vec<&String> = first
                        .keys()
                        .filter(|p| p.starts_with(from.as_str()))
                        .collect();
                    !under.is_empty()
                        && under.iter().all(|p| {
                            Repo::exact_pair(&ch, p)
                                == Pair::Exact(format!("{to}{}", &p[from.len()..]))
                        })
                        && !tau_h.keys().any(|p| p.starts_with(from.as_str()))
                })
                .collect();
            for (from, to) in &valid {
                let implied = valid.iter().any(|(f2, t2)| {
                    f2.len() < from.len()
                        && from.starts_with(f2.as_str())
                        && *to == format!("{t2}{}", &from[f2.len()..])
                });
                if !implied {
                    out.push((from.clone(), to.clone(), c.clone()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, parents: &[&str], t: i64, tree: &[(&str, &str)]) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            committer_time: t,
            author_time: t,
            tree: tree
                .iter()
                .map(|(p, b)| (p.to_string(), b.to_string()))
                .collect(),
        }
    }

    fn repo(cs: Vec<Commit>) -> Repo {
        Repo {
            algo: Algo::Sha1,
            commits: cs.into_iter().map(|c| (c.id.clone(), c)).collect(),
            refs: BTreeMap::new(),
            blobs: BTreeMap::new(),
        }
    }

    #[test]
    fn chains_follow_exact_renames_and_stop_at_groups() {
        let r = repo(vec![
            commit("c1", &[], 10, &[("a.rs", "b1"), ("x.rs", "b2")]),
            commit("c2", &["c1"], 20, &[("src/a.rs", "b1"), ("x.rs", "b2")]),
            commit("c3", &["c2"], 30, &[("lib/a.rs", "b1"), ("x.rs", "b2")]),
            commit(
                "c4",
                &["c3"],
                40,
                &[("lib/a.rs", "b1"), ("y1.rs", "b2"), ("y2.rs", "b2")],
            ),
        ]);
        let w = r.window_since("c1", "c4");
        assert_eq!(w, ["c2", "c3", "c4"]);
        assert_eq!(
            r.chain(&w, "a.rs"),
            Chain::Path {
                path: "lib/a.rs".into(),
                class: Class::Exact,
                score: 100
            }
        );
        assert_eq!(
            r.chain(&w, "x.rs"),
            Chain::Ambiguous(vec!["y1.rs".into(), "y2.rs".into()])
        );
        assert_eq!(r.chain(&w, "zzz"), Chain::NoStep);
        assert!(r.renamed_in_one_commit(&w, "a.rs", "src/a.rs"));
        assert!(!r.renamed_in_one_commit(&w, "a.rs", "lib/a.rs"));
        assert!(r.is_ancestor("c1", "c4"));
        assert!(!r.is_ancestor("c4", "c1"));
        // A deletion without a pair ends the chain, and a missing blob makes the inexact test unavailable.
        let r2 = repo(vec![
            commit("d1", &[], 1, &[("gone.rs", "g")]),
            commit("d2", &["d1"], 2, &[("new.rs", "n")]),
        ]);
        assert_eq!(r2.chain(&["d2".into()], "gone.rs"), Chain::Unavailable);
        let mut r3 = r2.clone();
        r3.blobs.insert("g".into(), b"a\n".to_vec());
        assert_eq!(
            r3.chain(&["d2".into()], "gone.rs"),
            Chain::Deleted("d2".into())
        );
    }

    #[test]
    fn committed_moves_need_every_entry() {
        let r = repo(vec![
            commit(
                "c1",
                &[],
                10,
                &[("docs/a.md", "1"), ("docs/b.md", "2"), ("top.md", "3")],
            ),
            commit(
                "c2",
                &["c1"],
                20,
                &[
                    ("arch/docs/a.md", "1"),
                    ("arch/docs/b.md", "2"),
                    ("top.md", "3"),
                ],
            ),
        ]);
        let w = r.window_since("c1", "c2");
        assert_eq!(
            r.committed_moves(&w, "c2"),
            vec![(
                "docs/".to_string(),
                "arch/docs/".to_string(),
                "c2".to_string()
            )]
        );
        let r2 = repo(vec![
            commit("c1", &[], 10, &[("docs/a.md", "1"), ("docs/b.md", "2")]),
            commit("c2", &["c1"], 20, &[("arch/a.md", "1"), ("docs/b.md", "2")]),
        ]);
        assert!(
            r2.committed_moves(&r2.window_since("c1", "c2"), "c2")
                .is_empty()
        );
    }

    fn lines(tag: &str, n: usize) -> String {
        (0..n)
            .map(|i| format!("{tag} line number {i} of this blob\n"))
            .collect()
    }

    /// A repository where c2 deletes `x.txt` (blob X) and holds `added` (path, blob) and, from c1, `hosts` rewritten
    /// to new blobs; blob contents are given.
    fn inexact_repo(
        x: &str,
        added: &[(&str, &str)],
        hosts: &[(&str, &str, &str)],
    ) -> (Repo, Vec<String>) {
        let mut t1: Vec<(&str, &str)> = vec![("x.txt", "X")];
        t1.extend(hosts.iter().map(|(p, _, _)| (*p, "H0")));
        let mut t2: Vec<(&str, &str)> = added.iter().map(|(p, _)| (*p, *p)).collect();
        for (p, _, _) in hosts {
            t2.push((p, p));
        }
        let mut r = repo(vec![
            commit("c1", &[], 1, &t1),
            commit("c2", &["c1"], 2, &t2),
        ]);
        r.blobs.insert("X".into(), x.as_bytes().to_vec());
        for (p, b) in added {
            r.blobs.insert((*p).into(), b.as_bytes().to_vec());
        }
        for (p, old, new) in hosts {
            r.blobs.insert("H0".into(), old.as_bytes().to_vec());
            r.blobs.insert((*p).into(), new.as_bytes().to_vec());
        }
        (r, vec!["c2".to_string()])
    }

    /// Every row of [F20 §5.11.4]'s inexact-pair table, in order.
    #[test]
    fn the_inexact_pair_rows() {
        let a = lines("alpha", 6);
        let b = lines("beta", 6);
        let x = format!("{a}{b}");
        // 1: a tiny X has no inexact pair: the chain ends deleted.
        let (r, w) = inexact_repo("one\ntwo\n", &[("y.txt", "one\ntwo\n!\n")], &[]);
        assert_eq!(r.chain(&w, "x.txt"), Chain::Deleted("c2".into()));
        // 2: a split into two pieces that each hold only X's lines and together hold all of them.
        let (r, w) = inexact_repo(&x, &[("p1.txt", &a), ("p2.txt", &b)], &[]);
        assert_eq!(
            r.chain(&w, "x.txt"),
            Chain::Split(vec!["p1.txt".into(), "p2.txt".into()])
        );
        // 3: exactly one added candidate with gs ≥ 90.
        let near = x.replacen(
            "beta line number 5 of this blob",
            "beta line number 5 of this blog",
            1,
        );
        let (r, w) = inexact_repo(
            &x,
            &[("far/y.txt", &near), ("z.txt", &lines("zeta", 9))],
            &[],
        );
        match r.chain(&w, "x.txt") {
            Chain::Path { path, class, score } => {
                assert_eq!((path.as_str(), class), ("far/y.txt", Class::Strong));
                assert!(score >= 90, "{score}");
            }
            c => panic!("{c:?}"),
        }
        // 4: the best has 50 ≤ gs < 90, 20 above the next, and the same directory.
        let mid = format!("{a}{}", lines("gamma", 3) + &lines("beta", 3));
        let (r, w) = inexact_repo(&x, &[("y.txt", &mid)], &[]);
        match r.chain(&w, "x.txt") {
            Chain::Path { path, class, score } => {
                assert_eq!((path.as_str(), class), ("y.txt", Class::Strong));
                assert!((50..90).contains(&score), "{score}");
            }
            c => panic!("{c:?}"),
        }
        // …but not in another directory with another basename: no strong pair, and nothing else holds.
        let (r, w) = inexact_repo(&x, &[("sub/other.txt", &mid)], &[]);
        assert!(
            !matches!(
                r.chain(&w, "x.txt"),
                Chain::Path {
                    class: Class::Strong,
                    ..
                }
            ),
            "row 4 needs a directory or basename corroboration"
        );
        // 5: merged into a host that gained all of X's lines and is at least twice as large.
        let host_old = lines("host", 14);
        let host_new = format!("{host_old}{x}");
        let (r, w) = inexact_repo(&x, &[], &[("host.txt", &host_old, &host_new)]);
        match r.chain(&w, "x.txt") {
            Chain::Merged(h, s) => {
                assert_eq!(h, "host.txt");
                assert_eq!(s, Ratio::int(1));
            }
            c => panic!("{c:?}"),
        }
        // 6: weak: the best added candidate has 20 ≤ gs < 50 and new-in-old ≥ 4/5.
        let part = lines("alpha", 5);
        let (r, w) = inexact_repo(&x, &[("q/w.txt", &part)], &[]);
        match r.chain(&w, "x.txt") {
            Chain::Path { path, class, score } => {
                assert_eq!((path.as_str(), class), ("q/w.txt", Class::Weak));
                assert!((20..50).contains(&score), "{score}");
            }
            c => panic!("{c:?}"),
        }
        // 7: none.
        let (r, w) = inexact_repo(&x, &[("u.txt", &lines("unrelated", 12))], &[]);
        assert_eq!(r.chain(&w, "x.txt"), Chain::Deleted("c2".into()));
        // A blob the history does not carry: unavailable.
        let (mut r, w) = inexact_repo(&x, &[("y.txt", &mid)], &[]);
        r.blobs.remove("y.txt");
        assert_eq!(r.chain(&w, "x.txt"), Chain::Unavailable);
    }

    #[test]
    fn merge_bases_are_maximal_common_ancestors() {
        let r = repo(vec![
            commit("a", &[], 1, &[]),
            commit("b", &["a"], 2, &[]),
            commit("c", &["a"], 3, &[]),
            commit("m", &["b", "c"], 4, &[]),
            commit("d", &["b"], 5, &[]),
        ]);
        assert_eq!(r.merge_bases("m", "d"), BTreeSet::from(["b".to_string()]));
        assert_eq!(r.merge_bases("c", "d"), BTreeSet::from(["a".to_string()]));
        let timed = r.window_timed("m", 4000u64 << 16, 0);
        assert!(
            timed
                .iter()
                .all(|c| r.commits[c].committer_time * 1000 >= 4000 - 86_400_000)
        );
    }
}
