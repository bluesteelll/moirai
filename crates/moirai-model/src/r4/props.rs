//! Property suites of the R4 model ([40 §8.3.2]).
//!
//! **Anchors** (P11's oracle side): a capture resolves `fresh` on its own content; after a pure insertion of unrelated
//! lines a quote or range is never `orphaned`, is `ambiguous` only where its quote recurs, and is otherwise at the
//! shifted span — every result at least as conservative as the moved span ([`at_least_as_conservative`]); long ranges
//! over repeated text move whole; a one-byte edit inside a unique quote is `edited` at its span.
//!
//! **Files**, over random operation sequences on a simulated tree with ground-truth identity — plain, git-committed and
//! directory moves, copies, removals, edits, quarantines, moves a reader tree or a hook observed as `PENDING` rows,
//! aliases, recorded and wrong `path_moves` entries, a case twin — under both values of `files.policy.auto`: no
//! automatic re-bind or guess points anywhere but at the file (P1) and none rests on content alone (I-F13), every
//! pure move of a file whose id the tree recorded is re-bound (P2), resolution is pure (P3), a second settle writes
//! nothing (P5), a quarantine that is restored before the settle writes nothing (P6), and every guess policy B applies
//! is of a class [F18 §5.4] lists. **Policy B's never-applied proposals**: a merge, a split, a directory moved with its
//! file replaced and a move made differently on another line are proposals with their own rendering and are never
//! applied, under either policy.

use crate::r4::anchor::{
    AResult, AState, Anchor, Consts, Content, Form, Kind, at_least_as_conservative, capture,
    resolve,
};
use crate::r4::cascade::{
    FileNode, FileObs, PClass, Params, PendingRow, PendingSource, Runtime, TreeRow, View,
    resolve_file,
};
use crate::r4::git::{Commit, Git, Head, Repo};
use crate::r4::path::Os;
use crate::r4::settle::{relink_of, settle};
use crate::r4::strings::State;
use crate::r4::tests::runner;
use crate::r4::text::{NText, atext, fingerprint, oid, xxh3};
use crate::r4::tree::{Fs, StatOut, TreeOp, VolumeCaps};
use crate::r4::uid::FileStatus;
use crate::r4::{if13_copy_rule, p1_ground_truth};
use crate::value::{Algo, MoveClass, PathMove, PathVal, Uid};
use proptest::prelude::*;
use std::collections::BTreeMap;

const WORDS: [&str; 8] = [
    "let x = compute();",
    "retry();",
    "}",
    "",
    "fn helper(a: u32) -> u32 {",
    "    a + 1",
    "// a comment line",
    "retry();",
];

const OTHER: [&str; 4] = ["alpha beta", "gamma delta", "epsilon", "zeta eta theta"];

fn text_of(lines: &[usize]) -> String {
    lines.iter().map(|&i| format!("{}\n", WORDS[i])).collect()
}

fn cap(form: Form, text: &str) -> Option<Anchor> {
    capture(
        Uid([2; 16]),
        Uid([7; 16]),
        &form,
        Some(text.as_bytes()),
        None,
        None,
        Algo::Sha1,
        &[],
        &Consts::DRAFT,
    )
    .ok()
    .map(|x| x.0)
}

fn res(a: &Anchor, text: &str) -> AResult {
    resolve(
        a,
        Content::Bytes(text.as_bytes()),
        Algo::Sha1,
        &Consts::DRAFT,
    )
}

fn moved_at(span: (u32, u32)) -> AResult {
    AResult {
        state: AState::Moved,
        span: Some(span),
        score: None,
        details: Vec::new(),
    }
}

/// The exact occurrences of a quote in a text's N (overlapping ones included).
fn occurrences(text: &str, q: &[u8]) -> usize {
    let n = NText::of(&atext(text.as_bytes()).unwrap()).n;
    if q.is_empty() || q.len() > n.len() {
        return 0;
    }
    (0..=n.len() - q.len())
        .filter(|&h| &n[h..h + q.len()] == q)
        .count()
}

#[test]
fn a_capture_resolves_fresh_on_its_own_content_and_shifts_with_an_insertion() {
    let strat = (
        proptest::collection::vec(0usize..WORDS.len(), 1..24),
        any::<usize>(),
        0usize..4,
        proptest::collection::vec(0usize..OTHER.len(), 0..5),
    );
    runner(256)
        .run(&strat, |(lines, pick, width, pre)| {
            let text = text_of(&lines);
            let n = NText::of(&atext(text.as_bytes()).unwrap()).len();
            prop_assume!(n >= 1);
            let l = 1 + pick % n;
            let m = (l + width).min(n);
            // A span of trivial lines with no window is refused: nothing to resolve.
            let Some(a) = cap(Form::Lines(l as u32, m as u32), &text) else {
                return Ok(());
            };
            let r = res(&a, &text);
            prop_assert_eq!(r.state, AState::Fresh, "{:?} on its own content", a.kind);
            prop_assert_eq!(r.span, a.hint);
            prop_assert_eq!(&r, &res(&a, &text));
            // An insertion of unrelated lines before the text.
            let k = pre.len() as u32;
            let head: String = pre.iter().map(|&i| format!("{}\n", OTHER[i])).collect();
            let shifted = format!("{head}{text}");
            let r2 = res(&a, &shifted);
            let (h1, h2) = a.hint.unwrap();
            let expected = moved_at((h1 + k, h2 + k));
            let fresh_at_hint = r2.state == AState::Fresh && r2.span == a.hint;
            if fresh_at_hint {
                // `fresh` at the unshifted hint only where the shifted text's hint lines still hash to the span hash
                // (the hint step, [F20 §6.2] step 1), never a hint-step match on another copy.
                let nt2 = NText::of(&atext(shifted.as_bytes()).unwrap());
                prop_assert_eq!(
                    Some(xxh3(nt2.st(h1 as usize, h2 as usize))),
                    a.span_hash,
                    "{:?} fresh at the hint after {} lines",
                    a.kind,
                    k
                );
            }
            prop_assert!(
                at_least_as_conservative(&r2, &expected) || fresh_at_hint,
                "{:?} {:?} after {} lines",
                r2,
                a.kind,
                k
            );
            if k == 0 {
                prop_assert_eq!(r2.state, AState::Fresh);
            }
            if matches!(a.kind, Kind::Quote | Kind::Range) {
                prop_assert_ne!(r2.state, AState::Orphaned, "{:?}", a.kind);
                if r2.state == AState::Ambiguous {
                    prop_assert!(occurrences(&shifted, &a.quote) >= 2, "{:?}", a);
                }
            }
            Ok(())
        })
        .unwrap();
}

const REPEAT: [&str; 3] = [
    "same line of repeated text in a long range",
    "x",
    "    retry(); // again and again",
];

#[test]
fn long_ranges_over_repeated_text_move_as_captured() {
    let strat = (
        0usize..REPEAT.len(),
        5usize..24,
        1usize..4,
        1usize..4,
        0usize..6,
        0usize..8,
    );
    runner(192)
        .run(&strat, |(kind, reps, pre, post, k, sub)| {
            let other = |i: usize| format!("{} {i}\n", OTHER[i % OTHER.len()]);
            let text: String = (0..pre).map(other).collect::<String>()
                + &format!("{}\n", REPEAT[kind]).repeat(reps)
                + &(pre..pre + post).map(other).collect::<String>();
            let head: String = (100..100 + k).map(other).collect();
            let shifted = format!("{head}{text}");
            let (l, m) = ((pre + 1) as u32, (pre + reps) as u32);
            let k = k as u32;
            // The whole block: a range whose end quote recurs inside it moves whole.
            let a = cap(Form::Lines(l, m), &text).expect("a range");
            prop_assert_eq!(a.kind, Kind::Range);
            prop_assert_eq!(res(&a, &text).state, AState::Fresh);
            let r = res(&a, &shifted);
            let want = if k == 0 { AState::Fresh } else { AState::Moved };
            prop_assert_eq!((r.state, r.span), (want, Some((l + k, m + k))));
            // A sub-range of five lines inside the block: the moved span, `fresh` at the hint where the hint's lines
            // still hash to the span hash (the hint step's definition, [F20 §6.2] step 1), or `ambiguous` when nothing
            // tells the copies apart; never another span, never `orphaned` or `edited`.
            let s = l + (sub % (reps - 4)) as u32;
            let e = s + 4;
            let b = cap(Form::Lines(s, e), &text).expect("a range");
            let own = res(&b, &text);
            prop_assert_eq!((own.state, own.span), (AState::Fresh, Some((s, e))));
            let r = res(&b, &shifted);
            prop_assert!(
                r.state == AState::Ambiguous
                    || (r.state == want && r.span == Some((s + k, e + k)))
                    || (r.state == AState::Fresh && r.span == Some((s, e))),
                "{:?} for {}-{} shifted {}",
                r,
                s,
                e,
                k
            );
            Ok(())
        })
        .unwrap();
}

const LOWER: [&str; 10] = [
    "alpha", "bravo", "delta", "hotel", "kilo", "lima", "oscar", "romeo", "sierra", "tango",
];
const UPPER: [&str; 6] = ["QWERTY", "ZXCVB", "PLMOK", "NBVCX", "HGFDS", "YTREW"];

#[test]
fn a_one_byte_edit_in_a_unique_quote_is_edited_at_its_span() {
    let strat = (
        proptest::collection::vec(0usize..LOWER.len(), 2..6),
        proptest::collection::vec((0usize..UPPER.len(), 0usize..UPPER.len()), 0..30),
        any::<usize>(),
        any::<usize>(),
        0u8..25,
        0usize..5,
    );
    runner(192)
        .run(&strat, |(words, filler, pos, mpos, off, k)| {
            let target: String = words
                .iter()
                .map(|&i| LOWER[i])
                .collect::<Vec<_>>()
                .join(" ");
            let fl: Vec<String> = filler
                .iter()
                .map(|(a, b)| format!("{} {}", UPPER[*a], UPPER[*b]))
                .collect();
            let at = pos % (fl.len() + 1);
            let lines = |t: &str| -> String {
                fl[..at]
                    .iter()
                    .map(|x| format!("{x}\n"))
                    .chain(std::iter::once(format!("{t}\n")))
                    .chain(fl[at..].iter().map(|x| format!("{x}\n")))
                    .collect()
            };
            let text = lines(&target);
            let l = (at + 1) as u32;
            let a = cap(Form::Lines(l, l), &text).expect("a quote");
            prop_assert_eq!(a.kind, Kind::Quote);
            // Substitute one letter of the quoted line by another letter.
            let mut t = target.into_bytes();
            let letters: Vec<usize> = (0..t.len()).filter(|&i| t[i] != b' ').collect();
            let i = letters[mpos % letters.len()];
            t[i] = b'a' + (t[i] - b'a' + 1 + off) % 26;
            let edited = lines(std::str::from_utf8(&t).unwrap());
            let head = "QQQQ ZZZZ\n".repeat(k);
            let r = res(&a, &format!("{head}{edited}"));
            let k = k as u32;
            prop_assert_eq!((r.state, r.span), (AState::Edited, Some((l + k, l + k))));
            prop_assert!(r.score.is_some());
            Ok(())
        })
        .unwrap();
}

/// A random operation on the simulated tree.
#[derive(Clone, Debug)]
enum Op {
    MoveFile(usize, usize),
    MoveDir(usize),
    Copy(usize),
    Remove(usize),
    Edit(usize),
    Quarantine(usize),
    /// A rename committed in git (`git mv` then a commit).
    GitMove(usize, usize),
    /// A pure move that another tree's settle (true) or this tree's evidence hook (false) recorded as a `PENDING` row.
    Observed(usize, usize, bool),
}

const DIRS: [&str; 3] = ["src", "docs", "lib"];

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..6, 0usize..3).prop_map(|(f, d)| Op::MoveFile(f, d)),
        (0usize..3).prop_map(Op::MoveDir),
        (0usize..6).prop_map(Op::Copy),
        (0usize..6).prop_map(Op::Remove),
        (0usize..6).prop_map(Op::Edit),
        (0usize..6).prop_map(Op::Quarantine),
        (0usize..6, 0usize..3).prop_map(|(f, d)| Op::GitMove(f, d)),
        (0usize..6, 0usize..3, any::<bool>()).prop_map(|(f, d, o)| Op::Observed(f, d, o)),
    ]
}

const ROOT: &str = "D:/w/repo";

fn body(i: usize) -> Vec<u8> {
    (0..8)
        .map(|l| format!("file {i} line {l} with enough text\n"))
        .collect::<String>()
        .into_bytes()
}

/// The path of the file whose id is `id`, if it is still in the tree.
fn truth(fs: &Fs, id: u64) -> Option<String> {
    fs.trees[ROOT]
        .files
        .iter()
        .find(|(_, f)| f.id == id)
        .map(|(p, _)| p.clone())
}

fn pm(class: MoveClass, from: &str, to: &str) -> PathMove {
    PathMove {
        hlc: 1,
        class,
        from: PathVal {
            root: "project".into(),
            text: from.into(),
        },
        to: PathVal {
            root: "project".into(),
            text: to.into(),
        },
        git: None,
    }
}

fn repo_of(commits: &[Commit], head: &str, blobs: BTreeMap<String, Vec<u8>>) -> Git {
    let mut g = Git::default();
    g.repos.insert(
        "r".into(),
        Repo {
            algo: Algo::Sha1,
            commits: commits.iter().map(|c| (c.id.clone(), c.clone())).collect(),
            refs: BTreeMap::from([("refs/heads/main".to_string(), head.to_string())]),
            blobs,
        },
    );
    g.heads.insert(
        ROOT.into(),
        ("r".into(), Head::Ref("refs/heads/main".into())),
    );
    g
}

fn settled_tree() -> TreeRow {
    TreeRow {
        first_settle_done: true,
        last_settle_hlc: 1u64 << 16,
        epochs: Vec::new(),
    }
}

/// The policy classes of [F18 §5.4].
const POLICY_CLASSES: [&str; 6] = [
    "file-id-edited",
    "prefix-strong",
    "git-pair",
    "edited+moved",
    "similarity",
    "argv",
];

/// The world a random operation sequence builds: the tree, the runtime rows, the nodes, the committed tree and history,
/// the recorded `path_moves`, each file's id, its committed path and whether it was edited.
struct World {
    fs: Fs,
    rt: Runtime,
    nodes: Vec<FileNode>,
    tau: BTreeMap<String, String>,
    commits: Vec<Commit>,
    moves: Vec<PathMove>,
    ids: Vec<u64>,
    committed: Vec<Option<String>>,
    edited: [bool; 6],
}

impl World {
    fn new(git_on: bool, aliased: &[bool], twin: bool, wrong_moves: bool) -> World {
        let mut w = World {
            fs: Fs::default(),
            rt: Runtime::default(),
            nodes: Vec::new(),
            tau: BTreeMap::new(),
            commits: Vec::new(),
            moves: Vec::new(),
            ids: Vec::new(),
            committed: Vec::new(),
            edited: [false; 6],
        };
        w.fs.ensure_tree(ROOT, "D", VolumeCaps::NTFS, Os::Windows);
        for i in 0..6 {
            let path = format!("{}/f{i}.rs", DIRS[i % 3]);
            w.fs.apply(
                ROOT,
                &TreeOp::Write {
                    path: path.clone(),
                    bytes: body(i),
                    btime_ns: Some(1_000 + i as i64),
                },
                1_000,
            )
            .unwrap();
            let StatOut::Present(s) = w.fs.trees[ROOT].stat(&path) else {
                unreachable!()
            };
            w.ids.push(s.id.id);
            w.tau.insert(path.clone(), format!("b{i}"));
            w.committed.push(Some(path.clone()));
            w.rt.fileobs.insert(
                (i as u32 + 1, ROOT.into()),
                FileObs {
                    file_id: Some(s.id.clone()),
                    parent_dir: Some(s.parent),
                    size: s.size,
                    mtime_ns: s.mtime_ns,
                    creation_ns: Some(s.btime_ns),
                    last_oid: Some(oid(Algo::Sha1, &body(i))),
                    verified_at: 1u64 << 16,
                    ..FileObs::default()
                },
            );
            w.nodes.push(FileNode {
                n: i as u32 + 1,
                root: "project".into(),
                path,
                oid: Some(oid(Algo::Sha1, &body(i))),
                bytes: Some(body(i).len() as u64),
                observed_git: git_on.then(|| "c0".to_string()),
                observed_blob: git_on.then(|| format!("b{i}")),
                relink: None,
                aliases: if aliased[i] {
                    vec![format!("old/f{i}.rs")]
                } else {
                    Vec::new()
                },
                status: FileStatus::Present,
                artifact_kind: None,
                tombstone: false,
                obs_hlc: 1u64 << 16,
                conflict: None,
                path_claim: false,
            });
        }
        if twin {
            // A second node registered at a case spelling of node 1's path, for other content: only node 1's content
            // is on disk, so the twin rule resolves node 1 and leaves the twin `missing`.
            let other = b"twin content that differs\n".to_vec();
            w.rt.fileobs.insert(
                (7, ROOT.into()),
                FileObs {
                    last_oid: Some(oid(Algo::Sha1, &other)),
                    ..FileObs::default()
                },
            );
            let first = w.nodes[0].clone();
            w.nodes.push(FileNode {
                n: 7,
                path: "Src/f0.rs".into(),
                oid: Some(oid(Algo::Sha1, &other)),
                aliases: Vec::new(),
                ..first
            });
        }
        w.rt.trees.insert(ROOT.into(), settled_tree());
        w.commits.push(Commit {
            id: "c0".into(),
            parents: Vec::new(),
            committer_time: 1,
            author_time: 1,
            tree: w.tau.clone(),
        });
        if wrong_moves {
            w.moves.push(pm(MoveClass::Explicit, "docs/", "lib/"));
            w.moves.push(pm(MoveClass::Observed, "src/", "docs/"));
        }
        w
    }

    fn at(&self, i: usize) -> Option<String> {
        truth(&self.fs, self.ids[i])
    }

    fn mv(&mut self, from: String, to: &str, now: i64) -> bool {
        self.fs
            .apply(
                ROOT,
                &TreeOp::Mv {
                    from,
                    to: to.to_string(),
                },
                now,
            )
            .is_ok()
    }

    fn apply(&mut self, step: usize, o: &Op, git_on: bool, now: i64) {
        match *o {
            Op::MoveFile(i, d) => {
                if let Some(p) = self.at(i) {
                    self.mv(p, &format!("{}/m{i}.rs", DIRS[d]), now);
                }
            }
            Op::MoveDir(d) => {
                let to = format!("moved/{}", DIRS[d]);
                if self.mv(DIRS[d].into(), &to, now) {
                    // `file mv` of a directory records its `explicit` entry.
                    self.moves.push(pm(
                        MoveClass::Explicit,
                        &format!("{}/", DIRS[d]),
                        &format!("{to}/"),
                    ));
                }
            }
            Op::Copy(i) => {
                if let Some(p) = self.at(i) {
                    let _ = self.fs.apply(
                        ROOT,
                        &TreeOp::Cp {
                            from: p,
                            to: format!("copy/c{i}.rs"),
                            keep_btime: false,
                        },
                        now,
                    );
                }
            }
            Op::Remove(i) => {
                if let Some(p) = self.at(i) {
                    let _ = self.fs.apply(ROOT, &TreeOp::Rm { path: p }, now);
                }
            }
            Op::Edit(i) => {
                if let Some(p) = self.at(i) {
                    let mut b = body(i);
                    b.extend_from_slice(b"an edit\n");
                    let _ = self.fs.apply(
                        ROOT,
                        &TreeOp::Write {
                            path: p,
                            bytes: b,
                            btime_ns: None,
                        },
                        now,
                    );
                    self.edited[i] = true;
                }
            }
            Op::Quarantine(i) => {
                // Moved aside and back before any settle: nothing to write for it.
                if let Some(p) = self.at(i)
                    && self.mv(p.clone(), "D:/scratch/q.rs", now)
                {
                    self.mv("D:/scratch/q.rs".into(), &p, now);
                }
            }
            Op::GitMove(i, d) => {
                let (Some(p), Some(c)) = (self.at(i), self.committed[i].clone()) else {
                    return;
                };
                let to = format!("{}/g{i}.rs", DIRS[d]);
                if !git_on || p != c || !self.mv(p, &to, now) {
                    return;
                }
                let blob = self.tau.remove(&c).expect("committed");
                self.tau.insert(to.clone(), blob);
                self.committed[i] = Some(to);
                let parent = self.commits.last().expect("c0").id.clone();
                self.commits.push(Commit {
                    id: format!("c{}", step + 1),
                    parents: vec![parent],
                    committer_time: 2,
                    author_time: 2,
                    tree: self.tau.clone(),
                });
            }
            Op::Observed(i, d, other_tree) => {
                let Some(p) = self.at(i) else { return };
                let to = format!("{}/p{i}.rs", DIRS[d]);
                if !self.mv(p, &to, now) {
                    return;
                }
                let content = self.fs.trees[ROOT].files[&to].bytes.clone();
                self.rt.pending.push(PendingRow {
                    n: i as u32 + 1,
                    tree: if other_tree {
                        "E:/other".into()
                    } else {
                        ROOT.into()
                    },
                    class: PClass::Exact,
                    source: if other_tree {
                        PendingSource::ReaderSettle
                    } else {
                        PendingSource::Hook
                    },
                    evidence: 3,
                    oid: Some(oid(Algo::Sha1, &content)),
                    from: self.nodes[i].path.clone(),
                    to,
                    hlc: (now as u64 / 1_000_000) << 16,
                });
            }
        }
    }
}

#[test]
fn no_wrong_rebind_and_moves_are_found() {
    let strat = (
        proptest::collection::vec(op(), 1..8),
        any::<bool>(),
        any::<bool>(),
        proptest::collection::vec(any::<bool>(), 6),
        any::<bool>(),
        any::<bool>(),
    );
    runner(160)
        .run(
            &strat,
            |(ops, git_on, policy_strong, aliased, twin, wrong_moves)| {
                let mut w = World::new(git_on, &aliased, twin, wrong_moves);
                let mut now = 2_000_000_000i64;
                for (step, o) in ops.iter().enumerate() {
                    now += 1_000_000;
                    w.apply(step, o, git_on, now);
                }
                let git = if git_on {
                    repo_of(&w.commits, &w.commits.last().unwrap().id, BTreeMap::new())
                } else {
                    Git::default()
                };
                let (fs, rt, nodes) = (&w.fs, &w.rt, &w.nodes);
                let view = View {
                    files: nodes.clone(),
                    moves: BTreeMap::from([("project".to_string(), w.moves.clone())]),
                    anchor_conflicts: Vec::new(),
                };
                let p = Params {
                    settle: true,
                    writer: true,
                    policy_strong,
                    ..Params::default()
                };
                for f in nodes {
                    let r = resolve_file(f, &view, fs, &git, rt, ROOT, &p);
                    let t = if f.n == 7 {
                        None
                    } else {
                        w.at(f.n as usize - 1)
                    };
                    // P1: an automatic re-bind or a guess points at the file; I-F13: never on content alone.
                    prop_assert!(
                        p1_ground_truth(&r, t.as_deref()),
                        "{:?} → {:?}, truth {:?}",
                        f.path,
                        r,
                        t
                    );
                    prop_assert!(
                        if13_copy_rule(&r, fs, ROOT, rt.fileobs.get(&(f.n, ROOT.to_string()))),
                        "{:?}",
                        r
                    );
                    // P3: pure.
                    prop_assert_eq!(&r, &resolve_file(f, &view, fs, &git, rt, ROOT, &p));
                    // P2: a pure move of a recorded file id is re-bound.
                    if let Some(t) = &t
                        && !w.edited[f.n as usize - 1]
                    {
                        if *t == f.path {
                            prop_assert_eq!(r.state, State::Ok, "{:?}", r);
                        } else {
                            prop_assert_eq!(
                                (r.state, r.at.as_deref()),
                                (State::MovedAuto, Some(t.as_str())),
                                "{:?}",
                                r
                            );
                        }
                    }
                }
                // The settle's writes: P1 for every re-bind, a policy class of [F18 §5.4] for every guess.
                let scope: Vec<u32> = nodes.iter().map(|f| f.n).collect();
                let out = settle(&scope, &view, fs, &git, rt, ROOT, &p, 5u64 << 16);
                for rb in &out.rebinds {
                    let t = w.at(rb.n as usize - 1);
                    prop_assert_eq!(Some(rb.to.as_str()), t.as_deref(), "{:?}", rb);
                    let parts: Vec<&str> = rb.relink.split('/').collect();
                    if parts[0] == "policy" {
                        prop_assert!(policy_strong, "{:?}", rb);
                        prop_assert!(POLICY_CLASSES.contains(&parts[1]), "{:?}", rb);
                    } else {
                        prop_assert!(
                            matches!(parts[0], "lazy" | "git" | "hook" | "explicit"),
                            "{:?}",
                            rb
                        );
                    }
                }
                // P5: after the settle's writes, a second settle writes nothing.
                let mut nodes2 = nodes.clone();
                for rb in &out.rebinds {
                    let f = nodes2.iter_mut().find(|f| f.n == rb.n).unwrap();
                    f.aliases.push(rb.from.clone());
                    f.path = rb.to.clone();
                    f.oid = rb.oid.clone();
                    f.relink = Some(rb.relink.clone());
                    f.observed_git = rb.observed_git.clone();
                    f.observed_blob = rb.observed_blob.clone();
                }
                let mut rt2 = rt.clone();
                rt2.fileobs.extend(out.fileobs.iter().cloned());
                rt2.fprint.extend(out.fprint.iter().cloned());
                rt2.dirmap.extend(out.dirmap.iter().cloned());
                rt2.prefixev.extend(out.prefixev.iter().cloned());
                let mut moves2 = w.moves.clone();
                moves2.extend(out.moves.iter().map(|m| {
                    let class = if m.committed {
                        MoveClass::Committed
                    } else {
                        MoveClass::Observed
                    };
                    pm(class, &m.from, &m.to)
                }));
                let view2 = View {
                    files: nodes2,
                    moves: BTreeMap::from([("project".to_string(), moves2)]),
                    anchor_conflicts: Vec::new(),
                };
                let again = settle(&scope, &view2, fs, &git, &rt2, ROOT, &p, 6u64 << 16);
                prop_assert!(again.rebinds.is_empty(), "{:?}", again.rebinds);
                prop_assert!(again.moves.is_empty(), "{:?}", again.moves);
                Ok(())
            },
        )
        .unwrap();
}

/// The proposals that must never be applied, with the detail each renders.
#[derive(Clone, Copy, Debug)]
enum Never {
    /// E6: merged into a host ([F20 §5.11.4] row 5): detail 18.
    Merged,
    /// E6: split into pieces (row 2): detail 17.
    Split,
    /// E3d: the directory moved and the file there was replaced (step 4): detail 21.
    DirReplaced,
    /// G4: a chain that starts at an alias only: detail 20.
    MovedDifferently,
}

fn lines_of(tag: &str, n: usize) -> String {
    (0..n)
        .map(|i| format!("{tag} line number {i} of this content\n"))
        .collect()
}

fn write(fs: &mut Fs, path: &str, bytes: &str, now: i64) {
    fs.apply(
        ROOT,
        &TreeOp::Write {
            path: path.into(),
            bytes: bytes.as_bytes().to_vec(),
            btime_ns: None,
        },
        now,
    )
    .unwrap();
}

fn commit(id: &str, ps: &[&str], tree: &[(&str, &str)]) -> Commit {
    Commit {
        id: id.into(),
        parents: ps.iter().map(|s| s.to_string()).collect(),
        committer_time: 1,
        author_time: 1,
        tree: tree
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    }
}

#[test]
fn policy_b_never_applies_merged_split_dir_replaced_or_moved_differently() {
    let strat = (
        prop_oneof![
            Just(Never::Merged),
            Just(Never::Split),
            Just(Never::DirReplaced),
            Just(Never::MovedDifferently),
        ],
        6usize..14,
        14usize..30,
        1usize..3,
        any::<bool>(),
    );
    runner(160)
        .run(&strat, |(case, n_old, n_host, changed, strong)| {
            let mut fs = Fs::default();
            fs.ensure_tree(ROOT, "D", VolumeCaps::NTFS, Os::Windows);
            let old = lines_of("old", n_old);
            let mut rt = Runtime::default();
            rt.trees.insert(ROOT.into(), settled_tree());
            let mut node = FileNode {
                n: 1,
                root: "project".into(),
                path: "x.txt".into(),
                oid: Some(oid(Algo::Sha1, old.as_bytes())),
                bytes: Some(old.len() as u64),
                observed_git: Some("c1".into()),
                observed_blob: Some("X".into()),
                relink: None,
                aliases: Vec::new(),
                status: FileStatus::Present,
                artifact_kind: None,
                tombstone: false,
                obs_hlc: 1u64 << 16,
                conflict: None,
                path_claim: false,
            };
            let mut blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
            blobs.insert("X".into(), old.clone().into_bytes());
            let (commits, expect): (Vec<Commit>, u8) = match case {
                Never::Merged => {
                    let host = lines_of("host", n_host);
                    let merged = format!("{host}{old}");
                    blobs.insert("H0".into(), host.into_bytes());
                    blobs.insert("H1".into(), merged.clone().into_bytes());
                    write(&mut fs, "host.txt", &merged, 1_000);
                    let c1 = commit("c1", &[], &[("x.txt", "X"), ("host.txt", "H0")]);
                    let c2 = commit("c2", &["c1"], &[("host.txt", "H1")]);
                    (vec![c1, c2], 18)
                }
                Never::Split => {
                    let half = n_old / 2;
                    let a: String = old.lines().take(half).map(|l| format!("{l}\n")).collect();
                    let b: String = old.lines().skip(half).map(|l| format!("{l}\n")).collect();
                    blobs.insert("A".into(), a.clone().into_bytes());
                    blobs.insert("B".into(), b.clone().into_bytes());
                    write(&mut fs, "p1.txt", &a, 1_000);
                    write(&mut fs, "p2.txt", &b, 1_000);
                    let c1 = commit("c1", &[], &[("x.txt", "X")]);
                    let c2 = commit("c2", &["c1"], &[("p1.txt", "A"), ("p2.txt", "B")]);
                    (vec![c1, c2], 17)
                }
                Never::MovedDifferently => {
                    // The node was observed on a lane at elsewhere.txt, once named x.txt; this line renamed x.txt.
                    node.path = "elsewhere.txt".into();
                    node.aliases = vec!["x.txt".into()];
                    node.observed_git = Some("lane".into());
                    write(&mut fs, "sub/y.txt", &old, 1_000);
                    let c1 = commit("c1", &[], &[("x.txt", "X")]);
                    let lane = commit("lane", &["c1"], &[("elsewhere.txt", "X")]);
                    let c2 = commit("c2", &["c1"], &[("sub/y.txt", "X")]);
                    (vec![c1, lane, c2], 20)
                }
                Never::DirReplaced => {
                    // docs/x.txt observed with its ids; docs/ renamed to arch/ and the file there rewritten by a new
                    // file (a new id) that still shares most lines.
                    node.path = "docs/x.txt".into();
                    node.observed_git = None;
                    node.observed_blob = None;
                    write(&mut fs, "docs/x.txt", &old, 1_000);
                    let StatOut::Present(s) = fs.trees[ROOT].stat("docs/x.txt") else {
                        unreachable!()
                    };
                    rt.fileobs.insert(
                        (1, ROOT.into()),
                        FileObs {
                            file_id: Some(s.id.clone()),
                            parent_dir: Some(s.parent),
                            size: s.size,
                            mtime_ns: s.mtime_ns,
                            creation_ns: Some(s.btime_ns),
                            last_oid: Some(oid(Algo::Sha1, old.as_bytes())),
                            verified_at: 1u64 << 16,
                            ..FileObs::default()
                        },
                    );
                    rt.fprint.insert(
                        oid(Algo::Sha1, old.as_bytes()),
                        fingerprint(old.as_bytes()).unwrap(),
                    );
                    let t = 2_000_000_000;
                    fs.apply(
                        ROOT,
                        &TreeOp::Mv {
                            from: "docs".into(),
                            to: "arch".into(),
                        },
                        t,
                    )
                    .unwrap();
                    fs.apply(
                        ROOT,
                        &TreeOp::Rm {
                            path: "arch/x.txt".into(),
                        },
                        t,
                    )
                    .unwrap();
                    let new: Vec<String> = old
                        .lines()
                        .enumerate()
                        .map(|(k, l)| {
                            if k < changed {
                                format!("rewritten line {k}")
                            } else {
                                l.to_string()
                            }
                        })
                        .collect();
                    write(&mut fs, "arch/x.txt", &(new.join("\n") + "\n"), t);
                    (Vec::new(), 21)
                }
            };
            let git = if commits.is_empty() {
                Git::default()
            } else {
                repo_of(&commits, "c2", blobs)
            };
            let view = View {
                files: vec![node.clone()],
                moves: BTreeMap::new(),
                anchor_conflicts: Vec::new(),
            };
            let p = Params {
                settle: true,
                writer: true,
                policy_strong: strong,
                ..Params::default()
            };
            let r = resolve_file(&node, &view, &fs, &git, &rt, ROOT, &p);
            prop_assert_eq!(
                (r.state, r.details[0].code, r.guess),
                (State::MovedNeedsConfirm, expect, None),
                "{:?}",
                r
            );
            prop_assert!(r.proposals.iter().all(|x| !x.auto), "{:?}", r.proposals);
            prop_assert_eq!(relink_of(&r), None);
            if let Never::Split = case {
                prop_assert_eq!(&r.candidates, &["p1.txt".to_string(), "p2.txt".to_string()]);
            }
            let out = settle(&[1], &view, &fs, &git, &rt, ROOT, &p, 5u64 << 16);
            prop_assert!(out.rebinds.is_empty(), "{:?}", out.rebinds);
            Ok(())
        })
        .unwrap();
}
