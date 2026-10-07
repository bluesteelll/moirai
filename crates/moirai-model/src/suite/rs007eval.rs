//! The RS-007 evaluation harness: one harness that any variant of RS-007's hierarchy rule is measured with
//! ([RULES/merge-table] RS-007, open points 15 and 35; [AR §11] OQ-A-6, OQ-A-11, OQ-A-12;
//! `docs/spec/reviews/wave-3d-verify.md`). A variant is a [`Rule`]: `Current` (RS-007 with the OQ-A-12 prototype),
//! `Wave3c` (RS-007 as wave 3c left it), `FromB` (the replay from B of spec sync 3) and `Cand`, the candidate slot
//! (`merge/cand.rs`), which behaves as `Current` until a candidate fills it.
//!
//! Every history runs on its own store per variant, to its end, and every merge-family command in it (a `sync`, a
//! cross-lane merge, a merge into `main` with its step-0 sync, a `--base` merge, both merges of a criss-cross, a
//! revert and a cherry-pick) is judged by the oracle ([`oracle`]), which no rule computes:
//! - a key is the hierarchy value (parent, order) of a task live in b, o and t; it is touched on side S when S's
//!   value differs from b's (state-based touch);
//! - a key touched on one side only must take that side's value, a key touched on neither keeps b's, a key touched on
//!   both with equal values takes it (the determined keys); a key touched on both with different values may take
//!   either side's value (MR-040 decides which; both are recorded);
//! - a staging with a `HierarchyCycle` is avoidable when some assignment under these constraints is a forest (brute
//!   force over the two-sided keys), genuine otherwise; a landing is wrong when a determined key lands at another value
//!   or a two-sided key at neither side's value.
//!
//! A secondary reading (CRIT-2's audit) also counts a key touched on a side when a commit of that side since B has it in
//! its net changeset ([`Tri::touch`]); it is reported next to the primary, state-based one, since the two disagree on
//! whole classes (a merge into `main` re-times the merged lane's moves; a revert after the reverted commit was merged).
//!
//! Two variants' logs of one history are then compared at their first divergence (outcome, resolution, or the
//! hierarchy of any branch), and the difference is judged by the oracle verdicts of both sides at that command, and
//! by the oracle faults of each whole history.
//!
//! The tests: [`rs007_eval_oracle_classifies_by_state`] (the oracle alone), [`rs007_eval_corpus_invariants`] and
//! [`rs007_eval_random_invariants`] (PR tier, under the thread's default rule, `MOIRAI_RS007_RULE`: only what every
//! acceptable rule must meet), and the ignored [`rs007_eval_report`] (one table of every metric for a variant, against
//! references), [`rs007_eval_trace`] (one corpus case or generated history, verbose) and [`rs007_eval_find`] (the
//! generated histories in which a variant's merge of a kind gets a verdict).

use super::*;
use crate::api::{Data, Outcome};
use crate::history::MergeData;
use crate::merge::{self, Counter, RULE, Rule};
use crate::state::{Alloc, State};
use crate::tx::{Position, Take};
use crate::vcs::Bases;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------------------------------------------------
// Histories.

/// The branches the generators use, by index: `main` and up to four lanes.
pub(crate) const NAMES: [&str; 5] = ["main", "lane/x", "lane/y", "lane/z", "lane/w"];

/// What a history does when a merge stages: resolve every violation's key to this side and continue (aborted when a
/// resolution is refused or the continue stages again), or abort at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// `--take ours` on every key.
    Ours,
    /// `--take theirs` on every key.
    Theirs,
    /// `--take base` on every key.
    Base,
    /// `merge --abort`.
    Abort,
}

/// A commit a `--base`, a revert or a cherry-pick names: a label a [`Ev::Mark`] set, or the k-th commit of the store's
/// pool (every branch tip seen after each event, in the order first seen; for a revert, of those in the branch's
/// history, for a cherry-pick, of those not in it), k taken modulo the pool's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CRef {
    /// A label.
    Label(&'static str),
    /// A pool index.
    Pool(usize),
}

/// A position under the parent ([`Position`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pos {
    /// `first`.
    First,
    /// `last`.
    Last,
}

/// One statement of an [`Ev::Tx`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum It {
    /// #n under #p (or to the root), no position.
    Mv(u32, Option<u32>),
    /// #n under #p (or to the root) at a position.
    MvAt(u32, Option<u32>, Pos),
    /// #n's priority.
    Prio(u32, i64),
}

/// One event of a history. Branch names are full names (`main`, `lane/x`, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ev {
    /// Creates lane `.0` (`lane/<name>`) at branch `.1`'s tip.
    Fork(&'static str, &'static str),
    /// On branch `.0`, #n under #p, or to the root, in its own commit.
    Mv(&'static str, u32, Option<u32>),
    /// The same at a position (a parent change that sets the order).
    MvAt(&'static str, u32, Option<u32>, Pos),
    /// On branch `.0`, several statements in one transaction (one commit).
    Tx(&'static str, Vec<It>),
    /// On branch `.0`, an order-only move: #n under its current parent there, at a position (skipped at the root).
    Reorder(&'static str, u32, Pos),
    /// On branch `.0`, #n's priority.
    Prio(&'static str, u32, i64),
    /// On branch `.0`, a delete of #n (`.2`: `POLICY REPARENT`).
    Del(&'static str, u32, bool),
    /// `sync` of a lane.
    Sync(&'static str, T),
    /// `merge src --into dst` (step 0's sync first when dst is `main`).
    Merge(&'static str, &'static str, T),
    /// `merge src --into dst --base C`.
    Based(&'static str, &'static str, CRef, T),
    /// `cherry-pick C --onto branch`.
    Pick(CRef, &'static str, T),
    /// `revert C --onto branch` (`--mainline 1` for a merge commit).
    Revert(CRef, &'static str, T),
    /// A criss-cross: a snapshot lane at `.0`'s tip, `merge .1 --into .0` (take `.2`), then `merge <snapshot> --into
    /// .1` (take `.3`), so the next merge between the two has two LCAs.
    Criss(&'static str, &'static str, T, T),
    /// Labels branch `.1`'s tip `.0`.
    Mark(&'static str, &'static str),
}

/// A history: the number of tasks (#1 to #n on `main` at the root), the lanes forked right after them, and its events.
#[derive(Clone, Debug)]
pub(crate) struct Hist {
    /// Tasks #1..=#tasks.
    pub tasks: u32,
    /// Lanes forked from `main` at the commit that creates the tasks.
    pub lanes: Vec<&'static str>,
    /// The events.
    pub ops: Vec<Ev>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Logs.

/// A hierarchy value: `Some((parent, order))` of a live node, `None` of an absent or deleted one.
pub(crate) type HV = Option<(Option<u32>, Option<String>)>;
/// The hierarchy values of tasks #1..=#n, by index n − 1.
pub(crate) type Hier = Vec<HV>;

/// The kind of a log entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    /// A move, a priority edit, a delete, a fork or a label.
    Edit,
    /// A `sync`.
    Sync,
    /// A merge of one lane into another.
    Cross,
    /// Step 0's sync of a merge into `main`.
    Step0,
    /// A merge into `main` (after its step 0).
    IntoMain,
    /// A `--base` merge.
    Based,
    /// One of a criss-cross's two merges.
    Criss,
    /// A cherry-pick.
    Pick,
    /// A revert.
    Revert,
}

impl Kind {
    /// The merge-family kinds, in report order.
    pub(crate) const MERGES: [Kind; 8] = [
        Kind::Sync,
        Kind::Cross,
        Kind::Step0,
        Kind::IntoMain,
        Kind::Based,
        Kind::Criss,
        Kind::Pick,
        Kind::Revert,
    ];

    fn name(self) -> &'static str {
        match self {
            Kind::Edit => "edit",
            Kind::Sync => "sync",
            Kind::Cross => "cross",
            Kind::Step0 => "step0",
            Kind::IntoMain => "into-main",
            Kind::Based => "based",
            Kind::Criss => "criss",
            Kind::Pick => "pick",
            Kind::Revert => "revert",
        }
    }
}

/// What a command did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Got {
    /// An edit that committed (or changed nothing).
    Ok,
    /// A merge-family command landed.
    Landed,
    /// Nothing to merge.
    UpToDate,
    /// Staged: `key class` of every violation, sorted.
    Staged(Vec<String>),
    /// Refused, with its error code.
    Refused(String),
    /// Not run (a reorder at the root, a revert or pick with no candidate commit).
    Skipped,
}

/// The oracle's verdict on one merge-family command ([`oracle`], [`judge`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Landed with every determined key at its value and every two-sided key at o's or t's (`took_o`, `took_t`).
    LandedOk { took_o: u32, took_t: u32 },
    /// Landed with determined keys elsewhere (by class) or two-sided keys at neither value; `order_only` when every
    /// wrong key has the expected parent.
    LandedWrong {
        one_sided: u32,
        untouched: u32,
        both_same: u32,
        neither: u32,
        order_only: bool,
    },
    /// Staged on a `HierarchyCycle` although a forest keeps every constraint; `on_determined` when a staged key is a
    /// determined one; `cand_wrong` when the staged candidate holds a determined key it did not stage elsewhere;
    /// `stuck` when no choice of o's or t's value for each staged key makes the candidate a forest (only `--take base`
    /// or a value resolves it).
    Avoidable {
        on_determined: bool,
        cand_wrong: bool,
        stuck: bool,
    },
    /// Staged on a `HierarchyCycle`, and no forest keeps every constraint (`cand_wrong`, `stuck` as for `Avoidable`).
    Genuine { cand_wrong: bool, stuck: bool },
    /// Staged with no `HierarchyCycle`.
    StagedOther,
    /// Refused.
    Refused,
    /// Up to date.
    UpToDate,
}

impl Verdict {
    /// An oracle fault: an avoidable staging or a wrong landing.
    pub(crate) fn fault(&self) -> bool {
        matches!(
            self,
            Verdict::Avoidable { .. } | Verdict::LandedWrong { .. }
        )
    }
}

/// The three states of a merge and its result, over the tasks, with what the store said about its base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tri {
    /// b.
    pub b: Hier,
    /// o (dst).
    pub o: Hier,
    /// t (src).
    pub t: Hier,
    /// The landed state, or the staged candidate.
    pub r: Hier,
    /// The base came from two or more LCAs (a virtual base, which the variant itself computes).
    pub virtual_base: bool,
    /// The base's commit is tip(dst) (a single LCA, or `--base`, naming it): RS-007's one-sided merge.
    pub one_sided: bool,
    /// A revert or cherry-pick of a commit with no hierarchy entry in its changeset.
    pub pick_no_hier: bool,
    /// The changeset touch of each task on o and on t: some commit of A(o) \ A(B) (resp. A(t) \ A(B); for a revert or
    /// cherry-pick, C itself) has the task's hierarchy key in its net changeset. The secondary oracle reading
    /// (CRIT-2's audit) counts a key touched on a side when it is state-touched or changeset-touched there.
    pub touch: Option<(Vec<bool>, Vec<bool>)>,
}

/// One merge-family command (or edit) of a history, as one variant's store ran it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The event's index in the history.
    pub op: usize,
    /// The kind.
    pub kind: Kind,
    /// The outcome.
    pub got: Got,
    /// For a staging: whether the history's resolution landed (`false`: re-staged or refused, then aborted).
    pub settled: Option<bool>,
    /// The oracle's verdict (merge-family commands that ran).
    pub verdict: Option<Verdict>,
    /// The secondary reading's verdict (touch = state-touch or changeset-touch, [`Tri::touch`]).
    pub verdict_ch: Option<Verdict>,
    /// The inputs the verdict was judged on.
    pub tri: Option<Box<Tri>>,
    /// The backstop's resets in the command (outside resolved-key derivations).
    pub backstop: usize,
    /// A candidate's repairs in the command.
    pub repairs: usize,
}

/// One history on one variant's store.
#[derive(Clone, Debug, Default)]
pub(crate) struct Log {
    /// Every entry, in order.
    pub entries: Vec<Entry>,
    /// After each event: every branch's hierarchy (name, tasks), in creation order.
    pub snaps: Vec<Vec<(String, Hier)>>,
    /// Wall time of the history, in seconds.
    pub secs: f64,
    /// The backstop's resets inside resolved-key derivations.
    pub derived_backstop: usize,
    /// The model panicked on the history (the message); the log is empty.
    pub panic: Option<String>,
}

impl Log {
    /// The oracle faults of the history.
    pub(crate) fn faults(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.verdict.as_ref().is_some_and(Verdict::fault))
            .count()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The oracle.

/// A key's class under the oracle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// The task is not live in all of b, o and t: not judged.
    NotJudged,
    /// Touched on neither side: keeps b's value.
    Untouched,
    /// Touched on o only: takes o's value.
    OnlyO,
    /// Touched on t only: takes t's value.
    OnlyT,
    /// Touched on both, to one value: takes it.
    BothSame,
    /// Touched on both, to different values: o's or t's.
    BothDiff,
}

/// The oracle's reading of b, o and t ([`oracle`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Oracle {
    /// Each task's class.
    pub class: Vec<Class>,
    /// Some task is not live in all three.
    pub partial: bool,
    /// Some assignment under the constraints is a forest over the judged tasks.
    pub forest: bool,
}

impl Oracle {
    /// The value a determined key must take; `None` for a two-sided or unjudged key.
    pub(crate) fn expected<'a>(
        &self,
        i: usize,
        b: &'a Hier,
        o: &'a Hier,
        t: &'a Hier,
    ) -> Option<&'a HV> {
        match self.class[i] {
            Class::Untouched => Some(&b[i]),
            Class::OnlyO | Class::BothSame => Some(&o[i]),
            Class::OnlyT => Some(&t[i]),
            Class::NotJudged | Class::BothDiff => None,
        }
    }
}

/// Whether the parents `p` (by task index; `None` for a root or for a parent outside the judged set) form a forest over
/// the judged tasks.
fn forest(p: &[Option<u32>], judged: &[bool]) -> bool {
    let n = p.len();
    for s in 0..n {
        if !judged[s] {
            continue;
        }
        let mut cur = p[s];
        for _ in 0..=n {
            match cur {
                Some(c) if (c as usize) >= 1 && (c as usize) <= n && judged[c as usize - 1] => {
                    if c as usize - 1 == s {
                        return false;
                    }
                    cur = p[c as usize - 1];
                }
                _ => break,
            }
        }
    }
    true
}

/// The oracle over b, o and t (state-based touch; no rule is read): each task's class, and whether some assignment of
/// the two-sided keys (each o's or t's value) with every determined key at its value is a forest.
pub(crate) fn oracle(b: &Hier, o: &Hier, t: &Hier) -> Oracle {
    oracle_touch(b, o, t, None)
}

/// [`oracle`], with a key also touched on a side where `touch` (o's mask, t's mask) says so (the secondary reading).
pub(crate) fn oracle_touch(
    b: &Hier,
    o: &Hier,
    t: &Hier,
    touch: Option<&(Vec<bool>, Vec<bool>)>,
) -> Oracle {
    let n = b.len();
    let mut class = vec![Class::NotJudged; n];
    for i in 0..n {
        if b[i].is_none() || o[i].is_none() || t[i].is_none() {
            continue;
        }
        let (co, ct) = touch.map_or((false, false), |(co, ct)| (co[i], ct[i]));
        let (to, tt) = (o[i] != b[i] || co, t[i] != b[i] || ct);
        class[i] = match (to, tt) {
            (false, false) => Class::Untouched,
            (true, false) => Class::OnlyO,
            (false, true) => Class::OnlyT,
            (true, true) if o[i] == t[i] => Class::BothSame,
            (true, true) => Class::BothDiff,
        };
    }
    let judged: Vec<bool> = class.iter().map(|c| *c != Class::NotJudged).collect();
    let two: Vec<usize> = (0..n).filter(|i| class[*i] == Class::BothDiff).collect();
    let par = |v: &HV| v.as_ref().and_then(|(p, _)| *p);
    let mut found = false;
    for mask in 0..(1u32 << two.len()) {
        let mut p: Vec<Option<u32>> = vec![None; n];
        for i in 0..n {
            p[i] = match class[i] {
                Class::NotJudged => None,
                Class::Untouched => par(&b[i]),
                Class::OnlyO | Class::BothSame => par(&o[i]),
                Class::OnlyT => par(&t[i]),
                Class::BothDiff => None,
            };
        }
        for (j, i) in two.iter().enumerate() {
            p[*i] = if mask >> j & 1 == 1 {
                par(&t[*i])
            } else {
                par(&o[*i])
            };
        }
        if forest(&p, &judged) {
            found = true;
            break;
        }
    }
    Oracle {
        class,
        partial: judged.iter().any(|j| !j),
        forest: found,
    }
}

/// The task index of a violation key `#n.parent`.
fn parent_key(k: &str) -> Option<usize> {
    let n: usize = k.strip_prefix('#')?.strip_suffix(".parent")?.parse().ok()?;
    n.checked_sub(1)
}

/// Judges one merge-family command by the oracle: `got` is what it did, `tri` its states and result, `viols` its
/// violations as (key, class).
pub(crate) fn judge(got: &Got, tri: &Tri, viols: &[(String, String)]) -> Verdict {
    judge_touch(got, tri, viols, None)
}

/// [`judge`] under [`oracle_touch`]'s reading.
pub(crate) fn judge_touch(
    got: &Got,
    tri: &Tri,
    viols: &[(String, String)],
    touch: Option<&(Vec<bool>, Vec<bool>)>,
) -> Verdict {
    let (b, o, t, r) = (&tri.b, &tri.o, &tri.t, &tri.r);
    let or = oracle_touch(b, o, t, touch);
    match got {
        Got::Landed => {
            let (mut one, mut unt, mut same, mut neither) = (0, 0, 0, 0);
            let (mut took_o, mut took_t) = (0, 0);
            let mut order_only = true;
            for i in 0..b.len() {
                let par = |v: &HV| v.as_ref().map(|(p, _)| *p);
                match or.class[i] {
                    Class::NotJudged => {}
                    Class::BothDiff => {
                        if r[i] == o[i] {
                            took_o += 1;
                        } else if r[i] == t[i] {
                            took_t += 1;
                        } else {
                            neither += 1;
                            order_only &= par(&r[i]) == par(&o[i]) || par(&r[i]) == par(&t[i]);
                        }
                    }
                    c => {
                        let e = or.expected(i, b, o, t).expect("a determined key");
                        if r[i] != *e {
                            order_only &= par(&r[i]) == par(e);
                            match c {
                                Class::Untouched => unt += 1,
                                Class::BothSame => same += 1,
                                _ => one += 1,
                            }
                        }
                    }
                }
            }
            if one + unt + same + neither == 0 {
                Verdict::LandedOk { took_o, took_t }
            } else {
                Verdict::LandedWrong {
                    one_sided: one,
                    untouched: unt,
                    both_same: same,
                    neither,
                    order_only,
                }
            }
        }
        Got::Staged(_) => {
            let hc: BTreeSet<usize> = viols
                .iter()
                .filter(|(_, c)| c == "HierarchyCycle")
                .filter_map(|(k, _)| parent_key(k))
                .collect();
            if !viols.iter().any(|(_, c)| c == "HierarchyCycle") {
                return Verdict::StagedOther;
            }
            let cand_wrong = (0..b.len())
                .any(|i| !hc.contains(&i) && or.expected(i, b, o, t).is_some_and(|e| r[i] != *e));
            // Whether `--take ours` or `--take theirs` on the staged keys can give a forest, the other keys as the
            // candidate holds them.
            let judged: Vec<bool> = or.class.iter().map(|c| *c != Class::NotJudged).collect();
            let staged: Vec<usize> = hc.iter().copied().filter(|i| judged[*i]).collect();
            let par = |v: &HV| v.as_ref().and_then(|(p, _)| *p);
            let stuck = !(0..(1u32 << staged.len())).any(|mask| {
                let mut p: Vec<Option<u32>> = r.iter().map(par).collect();
                for (j, i) in staged.iter().enumerate() {
                    p[*i] = if mask >> j & 1 == 1 {
                        par(&t[*i])
                    } else {
                        par(&o[*i])
                    };
                }
                forest(&p, &judged)
            });
            if or.forest {
                Verdict::Avoidable {
                    on_determined: hc.iter().any(|i| or.expected(*i, b, o, t).is_some()),
                    cand_wrong,
                    stuck,
                }
            } else {
                Verdict::Genuine { cand_wrong, stuck }
            }
        }
        Got::Refused(_) => Verdict::Refused,
        Got::UpToDate | Got::Ok | Got::Skipped => Verdict::UpToDate,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The executor.

/// Runs `f` on this thread under `rule`, restoring the thread's rule after, also when `f` panics.
pub(crate) fn under<R>(rule: Rule, f: impl FnOnce() -> R) -> R {
    struct Restore(Rule);
    impl Drop for Restore {
        fn drop(&mut self) {
            RULE.with(|r| r.set(self.0));
        }
    }
    let _restore = Restore(RULE.with(|r| r.replace(rule)));
    f()
}

fn fresh_on(b: &str) -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch_on(b)
    }
}

fn merge_cmd(src: &str, dst: &str, base: Option<String>) -> Cmd {
    Cmd::Merge {
        src: src.into(),
        into: Some(dst.into()),
        policy: None,
        strict: None,
        base,
        message: String::new(),
    }
}

fn take_of(t: T) -> Take {
    match t {
        T::Ours => Take::Ours,
        T::Theirs => Take::Theirs,
        T::Base | T::Abort => Take::Base,
    }
}

fn position(p: Pos) -> Position {
    match p {
        Pos::First => Position::First,
        Pos::Last => Position::Last,
    }
}

fn move_stmt(n: u32, p: Option<u32>, pos: Option<Pos>) -> Stmt {
    Stmt::Move {
        target: Target::Id(Nid(n)),
        under: p.map(|p| Target::Id(Nid(p))),
        position: pos.map(position),
    }
}

/// One history on one store under one rule.
struct Run {
    s: S,
    rule: Rule,
    tasks: u32,
    branches: Vec<String>,
    pool: Vec<u64>,
    labels: BTreeMap<&'static str, u64>,
    log: Log,
}

impl Run {
    fn new(h: &Hist, rule: Rule) -> Run {
        let mut s = S::base();
        s.ok(
            tx((1..=h.tasks)
                .map(|i| task(&format!("t{i}"), &format!("task {i}")))
                .collect()),
            orch(),
        );
        let mut branches = vec!["main".to_string()];
        for l in &h.lanes {
            s.ok(
                Cmd::BranchCreate {
                    name: l.strip_prefix("lane/").unwrap_or(l).into(),
                    from: Some("main".into()),
                    kind: None,
                },
                orch(),
            );
            branches.push(l.to_string());
        }
        let mut r = Run {
            s,
            rule,
            tasks: h.tasks,
            branches,
            pool: Vec::new(),
            labels: BTreeMap::new(),
            log: Log::default(),
        };
        r.note_pool();
        r
    }

    fn tip(&self, b: &str) -> Option<u64> {
        self.s.st.dag.live(b).and_then(|r| r.tip)
    }

    fn hier_at(&self, c: Option<u64>) -> Hier {
        let st = self.s.st.dag.state_at(c, &self.s.st.alloc);
        hier_of(&st, self.tasks)
    }

    fn note_pool(&mut self) {
        for b in &self.branches {
            if let Some(t) = self.s.st.dag.live(b).and_then(|r| r.tip)
                && !self.pool.contains(&t)
            {
                self.pool.push(t);
            }
        }
    }

    fn snap(&self) -> Vec<(String, Hier)> {
        self.branches
            .iter()
            .filter(|b| self.s.st.dag.live(b).is_some())
            .map(|b| (b.clone(), self.hier_at(self.tip(b))))
            .collect()
    }

    fn counts(&self) -> (usize, usize) {
        (
            merge::count(self.rule, Counter::Backstop),
            merge::count(self.rule, Counter::Repair),
        )
    }

    fn edit(&mut self, op: usize, b: &str, stmts: Vec<Stmt>) {
        let r = self.s.run(tx(stmts), fresh_on(b));
        let got = match r.outcome {
            Outcome::Ok | Outcome::Replayed | Outcome::Dry => Got::Ok,
            _ => Got::Refused(r.error.as_ref().map(|e| e.code.clone()).unwrap_or_default()),
        };
        self.push(op, Kind::Edit, got, None, None, None, (0, 0));
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        op: usize,
        kind: Kind,
        got: Got,
        settled: Option<bool>,
        verdict: Option<Verdict>,
        tri: Option<Tri>,
        d: (usize, usize),
    ) {
        self.log.entries.push(Entry {
            op,
            kind,
            got,
            settled,
            verdict,
            verdict_ch: None,
            tri: tri.map(Box::new),
            backstop: d.0,
            repairs: d.1,
        });
    }

    /// The tasks whose hierarchy key some commit of `side` \ `base` has in its net changeset.
    fn ch_touch(&self, side: &BTreeSet<u64>, base: &BTreeSet<u64>) -> Vec<bool> {
        let mut v = vec![false; self.tasks as usize];
        for c in side.difference(base) {
            for k in self.s.st.dag.commits[c].changeset.keys() {
                if let crate::state::Key::Node(n, crate::state::Aspect::Hierarchy) = k
                    && n.0 >= 1
                    && n.0 <= self.tasks
                {
                    v[n.0 as usize - 1] = true;
                }
            }
        }
        v
    }

    /// The oracle's inputs of a merge of `t` into `o` (`forced`: `--base`): b (the base of [F12 §4.3], a virtual base
    /// as the variant computes it), o, t, whether the base is virtual or the merge one-sided, and the changeset
    /// touches; the result is filled in by [`Run::record`].
    fn inputs(&self, o: Option<u64>, t: Option<u64>, forced: Option<u64>) -> Tri {
        let uid = |n: Nid| self.s.st.alloc.uid(n);
        let nid = |u| self.s.st.alloc.nid(u);
        let mut bs = Bases::new(&self.s.st.dag, &self.s.st.alloc, &uid, &nid);
        let base = bs.base(o, t, forced);
        let one_sided = match forced {
            Some(c) => Some(c) == o,
            None => base.lcas.len() <= 1 && base.lcas.first().copied() == o,
        };
        let dag = &self.s.st.dag;
        let touch = (
            self.ch_touch(&dag.ancestors(o), &base.anc),
            self.ch_touch(&dag.ancestors(t), &base.anc),
        );
        Tri {
            b: hier_of(&base.st, self.tasks),
            o: self.hier_at(o),
            t: self.hier_at(t),
            r: Vec::new(),
            virtual_base: base.virtual_base,
            one_sided,
            pick_no_hier: false,
            touch: Some(touch),
        }
    }

    /// Reads a merge-family reply, judges it over (b, o, t) and settles a staging with `take`; returns whether the
    /// command (after its resolution) landed.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        op: usize,
        kind: Kind,
        r: &Reply,
        dst: &str,
        inputs: Tri,
        take: T,
        before: (usize, usize),
    ) -> bool {
        let d = match &r.data {
            Data::Merge(d) => Some(d.as_ref()),
            _ => None,
        };
        let viols = |d: &MergeData| -> Vec<(String, String)> {
            let v = if d.violations.is_empty() {
                d.sync
                    .as_ref()
                    .filter(|s| s.outcome == "staged")
                    .map(|s| s.violations.clone())
                    .unwrap_or_default()
            } else {
                d.violations.clone()
            };
            v.into_iter().map(|v| (v.key, v.class)).collect()
        };
        let (got, result, vs) = match (&r.outcome, d) {
            (Outcome::Ok, Some(d)) if d.outcome == "up-to-date" => (Got::UpToDate, None, vec![]),
            (Outcome::Ok, _) => (Got::Landed, Some(self.tip(dst)), vec![]),
            (Outcome::Staged, Some(d)) => {
                let vs = viols(d);
                let mut keys: Vec<String> = vs.iter().map(|(k, c)| format!("{k} {c}")).collect();
                keys.sort();
                let g = d.staging_ref.clone().unwrap_or_default();
                (Got::Staged(keys), Some(self.tip(&g)), vs)
            }
            _ => (
                Got::Refused(r.error.as_ref().map(|e| e.code.clone()).unwrap_or_default()),
                None,
                vec![],
            ),
        };
        let tri = result.map(|c| Tri {
            r: self.hier_at(c),
            ..inputs
        });
        let verdict = match &tri {
            Some(tri) => judge(&got, tri, &vs),
            None => judge(&got, &dummy_tri(self.tasks), &vs),
        };
        let verdict_ch = tri
            .as_ref()
            .map(|tri| judge_touch(&got, tri, &vs, tri.touch.as_ref()));
        let mut settled = None;
        let mut landed = got == Got::Landed;
        if let (Got::Staged(_), Some(d)) = (&got, d) {
            let g = d.staging_ref.clone().unwrap_or_default();
            let keys: Vec<String> = vs.iter().map(|(k, _)| k.clone()).collect();
            let ok = self.settle(&g, &keys, take);
            settled = Some(ok);
            landed = ok;
        }
        let now = self.counts();
        self.push(
            op,
            kind,
            got,
            settled,
            Some(verdict),
            tri,
            (now.0 - before.0, now.1 - before.1),
        );
        if let Some(e) = self.log.entries.last_mut() {
            e.verdict_ch = verdict_ch;
        }
        landed
    }

    /// Resolves every key of staging `g` to `take` and continues; aborts when `take` is `Abort`, a resolution is
    /// refused, or the continue does not land. Returns whether it landed.
    fn settle(&mut self, g: &str, keys: &[String], take: T) -> bool {
        let rest = g.strip_prefix("merge/").expect("a staging ref");
        let (dst, src) = rest.split_once("/from/").expect("merge/<dst>/from/<src>");
        let (dst, src) = (dst.to_string(), src.to_string());
        let mut ok = take != T::Abort;
        if ok {
            let mut seen = BTreeSet::new();
            for k in keys.iter().filter(|k| *k != "-") {
                if !seen.insert(k.clone()) {
                    continue;
                }
                let r = self.s.run(
                    tx(vec![Stmt::Resolve {
                        key: k.clone(),
                        take: take_of(take),
                    }]),
                    fresh_on(g),
                );
                ok &= r.outcome == Outcome::Ok;
            }
        }
        if ok {
            let r = self.s.run(
                Cmd::MergeContinue {
                    src: Some(src.clone()),
                    into: Some(dst.clone()),
                },
                fresh_on(&dst),
            );
            if r.outcome == Outcome::Ok {
                return true;
            }
        }
        if self.s.st.dag.live(g).is_some() {
            let r = self.s.run(
                Cmd::MergeAbort {
                    src: Some(src),
                    into: Some(dst.clone()),
                },
                fresh_on(&dst),
            );
            assert!(r.outcome == Outcome::Ok, "abort refused: {:?}", r.error);
        }
        false
    }

    /// A merge of `src` into `dst` (`forced`: `--base`), with step 0's sync first when the store runs one.
    fn merge(&mut self, op: usize, kind: Kind, src: &str, dst: &str, forced: Option<u64>, take: T) {
        let (od, ts, tm) = (self.tip(dst), self.tip(src), self.tip("main"));
        let before = self.counts();
        let base = forced.map(|c| format!("s{c}"));
        let r = self.s.run(merge_cmd(src, dst, base.clone()), fresh_on(dst));
        let d = match &r.data {
            Data::Merge(d) => Some(d.as_ref().clone()),
            _ => None,
        };
        let Some(sync) = d.as_ref().and_then(|d| d.sync.clone()) else {
            let inputs = self.inputs(od, ts, forced);
            self.record(op, kind, &r, dst, inputs, take, before);
            return;
        };
        // Step 0: `sync src` (o = src's tip, t = main's).
        let inputs = self.inputs(ts, tm, None);
        if sync.outcome == "staged" {
            let landed = self.record(op, Kind::Step0, &r, src, inputs, take, before);
            if !landed {
                return;
            }
            // The step-0 sync landed by resolution: the merge into `main` runs again, now with no step 0.
            let (od, ts) = (self.tip(dst), self.tip(src));
            let before = self.counts();
            let r = self.s.run(merge_cmd(src, dst, base), fresh_on(dst));
            let inputs = self.inputs(od, ts, forced);
            self.record(op, kind, &r, dst, inputs, take, before);
            return;
        }
        // Step 0 landed: its entry, judged on the lane's new tip, then the merge itself.
        let sc = sync.commit;
        let tri = Tri {
            r: self.hier_at(sc),
            ..inputs
        };
        let vs: Vec<(String, String)> = Vec::new();
        let verdict = judge(&Got::Landed, &tri, &vs);
        let verdict_ch = judge_touch(&Got::Landed, &tri, &vs, tri.touch.as_ref());
        self.push(
            op,
            Kind::Step0,
            Got::Landed,
            None,
            Some(verdict),
            Some(tri),
            (0, 0),
        );
        if let Some(e) = self.log.entries.last_mut() {
            e.verdict_ch = Some(verdict_ch);
        }
        let inputs = self.inputs(od, sc, forced);
        self.record(op, kind, &r, dst, inputs, take, before);
    }

    fn resolve_c(&self, c: CRef, onto: &str, revert: bool) -> Option<u64> {
        match c {
            CRef::Label(l) => self.labels.get(l).copied(),
            CRef::Pool(k) => {
                let held = self.s.st.dag.ancestors(self.tip(onto));
                let cands: Vec<u64> = self
                    .pool
                    .iter()
                    .copied()
                    .filter(|c| held.contains(c) == revert)
                    .collect();
                (!cands.is_empty()).then(|| cands[k % cands.len()])
            }
        }
    }

    fn pick(&mut self, op: usize, c: CRef, onto: &str, take: T, revert: bool) {
        let kind = if revert { Kind::Revert } else { Kind::Pick };
        let Some(c) = self.resolve_c(c, onto, revert) else {
            self.push(op, kind, Got::Skipped, None, None, None, (0, 0));
            return;
        };
        let o = self.tip(onto);
        let before = self.counts();
        let x = &self.s.st.dag.commits[&c];
        let two = x.parents.len() == 2;
        let no_hier = !x.changeset.keys().any(|k| {
            matches!(
                k,
                crate::state::Key::Node(_, crate::state::Aspect::Hierarchy)
            )
        });
        let cmd = if revert {
            Cmd::Revert {
                commit: format!("s{c}"),
                onto: Some(onto.into()),
                mainline: two.then_some(1),
                message: String::new(),
            }
        } else {
            Cmd::CherryPick {
                commit: format!("s{c}"),
                onto: Some(onto.into()),
                message: String::new(),
            }
        };
        let (b, t) = crate::history::pick_states(&self.s.st.dag, &self.s.st.alloc, c, revert);
        // The changeset touch: dst's commits since the base's commit (C for a revert, its first parent for a pick),
        // and C itself on the src side.
        let dag = &self.s.st.dag;
        let base_at = if revert {
            Some(c)
        } else {
            dag.commits[&c].parents.first().copied()
        };
        let touch = (
            self.ch_touch(&dag.ancestors(o), &dag.ancestors(base_at)),
            self.ch_touch(&BTreeSet::from([c]), &BTreeSet::new()),
        );
        let inputs = Tri {
            b: hier_of(&b, self.tasks),
            o: self.hier_at(o),
            t: hier_of(&t, self.tasks),
            r: Vec::new(),
            virtual_base: false,
            one_sided: false,
            pick_no_hier: no_hier,
            touch: Some(touch),
        };
        let r = self.s.run(cmd, fresh_on(onto));
        self.record(op, kind, &r, onto, inputs, take, before);
    }

    fn event(&mut self, op: usize, ev: &Ev) {
        match ev {
            Ev::Fork(name, from) => {
                let r = self.s.run(
                    Cmd::BranchCreate {
                        name: name.strip_prefix("lane/").unwrap_or(name).into(),
                        from: Some((*from).into()),
                        kind: None,
                    },
                    orch(),
                );
                if r.outcome == Outcome::Ok && !self.branches.iter().any(|b| b == name) {
                    self.branches.push(name.to_string());
                }
                self.push(op, Kind::Edit, Got::Ok, None, None, None, (0, 0));
            }
            Ev::Mv(b, n, p) => self.edit(op, b, vec![move_stmt(*n, *p, None)]),
            Ev::MvAt(b, n, p, pos) => self.edit(op, b, vec![move_stmt(*n, *p, Some(*pos))]),
            Ev::Tx(b, items) => {
                let stmts = items
                    .iter()
                    .map(|i| match i {
                        It::Mv(n, p) => move_stmt(*n, *p, None),
                        It::MvAt(n, p, pos) => move_stmt(*n, *p, Some(*pos)),
                        It::Prio(n, k) => set(*n, &[("priority", P::Int(*k))]),
                    })
                    .collect();
                self.edit(op, b, stmts);
            }
            Ev::Reorder(b, n, pos) => {
                let cur = self.hier_at(self.tip(b));
                match cur.get(*n as usize - 1).cloned().flatten() {
                    Some((Some(p), _)) => {
                        self.edit(op, b, vec![move_stmt(*n, Some(p), Some(*pos))])
                    }
                    _ => self.push(op, Kind::Edit, Got::Skipped, None, None, None, (0, 0)),
                }
            }
            Ev::Prio(b, n, k) => self.edit(op, b, vec![set(*n, &[("priority", P::Int(*k))])]),
            Ev::Del(b, n, reparent) => self.edit(
                op,
                b,
                vec![Stmt::Delete {
                    target: Target::Id(Nid(*n)),
                    policy: reparent.then(|| "reparent".to_string()),
                    replaced_by: None,
                    release: false,
                    reason: Some("gone".into()),
                }],
            ),
            Ev::Sync(l, take) => {
                let (o, t) = (self.tip(l), self.tip("main"));
                let before = self.counts();
                let r = self.s.run(
                    Cmd::Sync {
                        lane: Some((*l).into()),
                        check: false,
                    },
                    fresh_on(l),
                );
                let inputs = self.inputs(o, t, None);
                self.record(op, Kind::Sync, &r, l, inputs, *take, before);
            }
            Ev::Merge(src, dst, take) => {
                let kind = if *dst == "main" {
                    Kind::IntoMain
                } else {
                    Kind::Cross
                };
                self.merge(op, kind, src, dst, None, *take);
            }
            Ev::Based(src, dst, c, take) => match match c {
                CRef::Pool(k) => (!self.pool.is_empty()).then(|| self.pool[k % self.pool.len()]),
                CRef::Label(l) => self.labels.get(l).copied(),
            } {
                Some(c) => self.merge(op, Kind::Based, src, dst, Some(c), *take),
                None => self.push(op, Kind::Based, Got::Skipped, None, None, None, (0, 0)),
            },
            Ev::Pick(c, onto, take) => self.pick(op, *c, onto, *take, false),
            Ev::Revert(c, onto, take) => self.pick(op, *c, onto, *take, true),
            Ev::Criss(l1, l2, t1, t2) => {
                let name = format!("lane/c{op}");
                let r = self.s.run(
                    Cmd::BranchCreate {
                        name: name.strip_prefix("lane/").unwrap().into(),
                        from: Some((*l1).into()),
                        kind: None,
                    },
                    orch(),
                );
                if r.outcome != Outcome::Ok {
                    self.push(op, Kind::Criss, Got::Skipped, None, None, None, (0, 0));
                    return;
                }
                self.branches.push(name.clone());
                self.merge(op, Kind::Criss, l2, l1, None, *t1);
                self.merge(op, Kind::Criss, &name, l2, None, *t2);
            }
            Ev::Mark(label, b) => {
                if let Some(t) = self.tip(b) {
                    self.labels.insert(label, t);
                }
                self.push(op, Kind::Edit, Got::Ok, None, None, None, (0, 0));
            }
        }
    }
}

fn dummy_tri(n: u32) -> Tri {
    Tri {
        b: vec![None; n as usize],
        o: vec![None; n as usize],
        t: vec![None; n as usize],
        r: vec![None; n as usize],
        virtual_base: false,
        one_sided: false,
        pick_no_hier: false,
        touch: None,
    }
}

/// The hierarchy of tasks #1..=#n in a state.
pub(crate) fn hier_of(st: &State, n: u32) -> Hier {
    (1..=n)
        .map(|i| match st.nodes.get(&Nid(i)) {
            Some(x) if x.live() => Some((x.parent.map(|p| p.0), x.order.clone())),
            _ => None,
        })
        .collect()
}

/// Runs history `h` on a new store under `rule`, to its end.
pub(crate) fn run(h: &Hist, rule: Rule) -> Log {
    under(rule, || {
        let t0 = std::time::Instant::now();
        let d0 = merge::count_derived(rule, Counter::Backstop);
        let mut r = Run::new(h, rule);
        for (i, ev) in h.ops.iter().enumerate() {
            r.event(i, ev);
            r.note_pool();
            let snap = r.snap();
            r.log.snaps.push(snap);
        }
        r.log.secs = t0.elapsed().as_secs_f64();
        r.log.derived_backstop = merge::count_derived(rule, Counter::Backstop) - d0;
        r.log
    })
}

/// Runs history `h` under each of `rules`, a panic recorded as the log's `panic` instead of failing the caller.
fn run_caught(h: &Hist, rule: Rule) -> Log {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(h, rule))) {
        Ok(l) => l,
        Err(e) => Log {
            panic: Some(
                e.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into()),
            ),
            ..Log::default()
        },
    }
}

/// Runs every history under every rule on `threads` threads (each history's stores on one thread); the result is per
/// history, per rule in `rules`' order.
pub(crate) fn run_all(hists: &[Hist], rules: &[Rule], threads: usize) -> Vec<Vec<Log>> {
    let threads = threads.max(1);
    std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|tid| {
                std::thread::Builder::new()
                    .stack_size(256 << 20)
                    .spawn_scoped(sc, move || {
                        hists
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| i % threads == tid)
                            .map(|(i, h)| {
                                (
                                    i,
                                    rules.iter().map(|r| run_caught(h, *r)).collect::<Vec<_>>(),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .expect("a worker thread")
            })
            .collect();
        let mut all: Vec<Vec<Log>> = vec![Vec::new(); hists.len()];
        for h in handles {
            for (i, logs) in h.join().expect("a worker") {
                all[i] = logs;
            }
        }
        all
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Comparison of two variants' logs of one history.

/// Where two variants' runs of one history first differ (outcome, resolution, or any branch's hierarchy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Div {
    /// No divergence.
    Same,
    /// The variant stages or is refused where the reference lands.
    Worse,
    /// The variant lands where the reference stages or is refused.
    Better,
    /// Both land (directly or by the history's resolution), with different parents on some branch.
    BothLand,
    /// Both land, with the same parents and different orders.
    BothLandOrder,
    /// Both stage, on different keys.
    BothStage,
    /// The same staging; the variant's resolution aborts where the reference's lands.
    ResolveWorse,
    /// The same staging; the variant's resolution lands where the reference's aborts.
    ResolveBetter,
    /// Anything else (refused against staged, a different number of commands, ...).
    Other,
}

impl Div {
    const ALL: [Div; 9] = [
        Div::Same,
        Div::Worse,
        Div::Better,
        Div::BothLand,
        Div::BothLandOrder,
        Div::BothStage,
        Div::ResolveWorse,
        Div::ResolveBetter,
        Div::Other,
    ];

    fn name(self) -> &'static str {
        match self {
            Div::Same => "same (no divergence)",
            Div::Worse => "worse: stages/refused where ref lands",
            Div::Better => "better: lands where ref stages/refused",
            Div::BothLand => "both land, parents differ",
            Div::BothLandOrder => "both land, only orders differ",
            Div::BothStage => "both stage, different keys",
            Div::ResolveWorse => "same staging, resolution aborts where ref's lands",
            Div::ResolveBetter => "same staging, resolution lands where ref's aborts",
            Div::Other => "other divergence",
        }
    }
}

/// The comparison of a variant's log `a` with a reference's log `r` of one history.
#[derive(Clone, Debug)]
pub(crate) struct Cmp {
    /// The first divergence.
    pub div: Div,
    /// The event it is at.
    pub at: Option<usize>,
    /// The variant has an oracle fault at that event.
    pub a_fault: bool,
    /// The reference has an oracle fault at that event.
    pub r_fault: bool,
    /// Oracle faults over the whole history: the variant's, the reference's.
    pub faults: (usize, usize),
    /// A one-line description.
    pub msg: String,
}

fn brief(es: &[&Entry]) -> String {
    es.iter()
        .map(|e| {
            let v = e.verdict.as_ref().map(verdict_text).unwrap_or_default();
            let s = match e.settled {
                Some(true) => " -> resolved, landed",
                Some(false) => " -> resolution aborted",
                None => "",
            };
            format!("{}:{}{s} [{v}]", e.kind.name(), got_text(&e.got))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Compares two logs of history `h` ([`Div`]).
pub(crate) fn compare(h: &Hist, a: &Log, r: &Log) -> Cmp {
    let faults = (a.faults(), r.faults());
    if a.panic.is_some() || r.panic.is_some() {
        return Cmp {
            div: Div::Other,
            at: None,
            a_fault: false,
            r_fault: false,
            faults,
            msg: format!("panic: {:?} / {:?}", a.panic, r.panic),
        };
    }
    for i in 0..h.ops.len() {
        let ea: Vec<&Entry> = a.entries.iter().filter(|e| e.op == i).collect();
        let er: Vec<&Entry> = r.entries.iter().filter(|e| e.op == i).collect();
        let same = ea.len() == er.len()
            && ea
                .iter()
                .zip(&er)
                .all(|(x, y)| x.kind == y.kind && x.got == y.got && x.settled == y.settled);
        if same && a.snaps.get(i) == r.snaps.get(i) {
            continue;
        }
        let fault = |es: &[&Entry]| {
            es.iter()
                .any(|e| e.verdict.as_ref().is_some_and(Verdict::fault))
        };
        let mut div = None;
        for (x, y) in ea.iter().zip(&er) {
            if x.kind != y.kind {
                div = Some(Div::Other);
            } else if x.got != y.got {
                div = Some(match (&x.got, &y.got) {
                    (Got::Staged(_) | Got::Refused(_), Got::Landed) => Div::Worse,
                    (Got::Landed, Got::Staged(_) | Got::Refused(_)) => Div::Better,
                    (Got::Staged(_), Got::Staged(_)) => Div::BothStage,
                    _ => Div::Other,
                });
            } else if x.settled != y.settled {
                div = Some(match (x.settled, y.settled) {
                    (Some(false), Some(true)) => Div::ResolveWorse,
                    (Some(true), Some(false)) => Div::ResolveBetter,
                    _ => Div::Other,
                });
            }
            if div.is_some() {
                break;
            }
        }
        let div = div.unwrap_or_else(|| {
            if ea.len() != er.len() {
                return Div::Other;
            }
            let par = |s: Option<&Vec<(String, Hier)>>| {
                s.map(|s| {
                    s.iter()
                        .map(|(b, h)| {
                            (
                                b.clone(),
                                h.iter()
                                    .map(|v| v.as_ref().map(|x| x.0))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            };
            if par(a.snaps.get(i)) == par(r.snaps.get(i)) {
                Div::BothLandOrder
            } else {
                Div::BothLand
            }
        });
        return Cmp {
            div,
            at: Some(i),
            a_fault: fault(&ea),
            r_fault: fault(&er),
            faults,
            msg: format!("[{i}] {:?}: {} || ref {}", h.ops[i], brief(&ea), brief(&er)),
        };
    }
    Cmp {
        div: Div::Same,
        at: None,
        a_fault: false,
        r_fault: false,
        faults,
        msg: String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Metrics.

/// One variant's absolute counts over merge-family commands (all kinds, or one).
#[derive(Clone, Debug, Default)]
pub(crate) struct KStats {
    pub n: usize,
    pub landed: usize,
    pub staged: usize,
    pub refused: usize,
    pub up_to_date: usize,
    pub genuine: usize,
    pub avoidable: usize,
    pub avoidable_det: usize,
    pub staged_other: usize,
    pub cand_wrong: usize,
    pub stuck: usize,
    pub wrong: usize,
    pub wrong_one: usize,
    pub wrong_untouched: usize,
    pub wrong_same: usize,
    pub wrong_neither: usize,
    pub wrong_order_only: usize,
    pub took_o: usize,
    pub took_t: usize,
    pub settle_aborted: usize,
    pub one_sided: usize,
    pub one_sided_not_t: usize,
    pub pick_no_hier: usize,
    pub pick_no_hier_not_o: usize,
    pub backstop: usize,
    pub repairs: usize,
    pub virtual_n: usize,
    pub partial_n: usize,
    pub avoidable_partial: usize,
    pub wrong_partial: usize,
    pub avoidable_virtual: usize,
    pub wrong_virtual: usize,
    pub genuine_ch: usize,
    pub avoidable_ch: usize,
    pub wrong_ch: usize,
}

impl KStats {
    fn add(&mut self, e: &Entry) {
        if e.kind == Kind::Edit || e.got == Got::Skipped {
            return;
        }
        self.n += 1;
        self.backstop += e.backstop;
        self.repairs += e.repairs;
        match &e.got {
            Got::Landed => self.landed += 1,
            Got::Staged(_) => self.staged += 1,
            Got::Refused(_) => self.refused += 1,
            _ => self.up_to_date += 1,
        }
        if e.settled == Some(false) {
            self.settle_aborted += 1;
        }
        if let Some(t) = &e.tri {
            let partial =
                t.b.iter()
                    .zip(&t.o)
                    .zip(&t.t)
                    .any(|((b, o), t)| b.is_none() || o.is_none() || t.is_none());
            self.virtual_n += t.virtual_base as usize;
            self.partial_n += partial as usize;
            let avoidable = matches!(e.verdict, Some(Verdict::Avoidable { .. }));
            let wrong = matches!(e.verdict, Some(Verdict::LandedWrong { .. }));
            self.avoidable_partial += (avoidable && partial) as usize;
            self.wrong_partial += (wrong && partial) as usize;
            self.avoidable_virtual += (avoidable && t.virtual_base) as usize;
            self.wrong_virtual += (wrong && t.virtual_base) as usize;
            let hc =
                matches!(&e.got, Got::Staged(k) if k.iter().any(|k| k.ends_with("HierarchyCycle")));
            if t.one_sided && matches!(e.got, Got::Landed | Got::Staged(_)) {
                self.one_sided += 1;
                if hc || (e.got == Got::Landed && !judged_eq(&t.r, &t.t, t)) {
                    self.one_sided_not_t += 1;
                }
            }
            if t.pick_no_hier && matches!(e.got, Got::Landed | Got::Staged(_)) {
                self.pick_no_hier += 1;
                if hc || (e.got == Got::Landed && !judged_eq(&t.r, &t.o, t)) {
                    self.pick_no_hier_not_o += 1;
                }
            }
        }
        match &e.verdict_ch {
            Some(Verdict::Genuine { .. }) => self.genuine_ch += 1,
            Some(Verdict::Avoidable { .. }) => self.avoidable_ch += 1,
            Some(Verdict::LandedWrong { .. }) => self.wrong_ch += 1,
            _ => {}
        }
        match &e.verdict {
            Some(Verdict::LandedOk { took_o, took_t }) => {
                self.took_o += *took_o as usize;
                self.took_t += *took_t as usize;
            }
            Some(Verdict::LandedWrong {
                one_sided,
                untouched,
                both_same,
                neither,
                order_only,
            }) => {
                self.wrong += 1;
                self.wrong_one += (*one_sided > 0) as usize;
                self.wrong_untouched += (*untouched > 0) as usize;
                self.wrong_same += (*both_same > 0) as usize;
                self.wrong_neither += (*neither > 0) as usize;
                self.wrong_order_only += *order_only as usize;
            }
            Some(Verdict::Avoidable {
                on_determined,
                cand_wrong,
                stuck,
            }) => {
                self.avoidable += 1;
                self.avoidable_det += *on_determined as usize;
                self.cand_wrong += *cand_wrong as usize;
                self.stuck += *stuck as usize;
            }
            Some(Verdict::Genuine { cand_wrong, stuck }) => {
                self.genuine += 1;
                self.cand_wrong += *cand_wrong as usize;
                self.stuck += *stuck as usize;
            }
            Some(Verdict::StagedOther) => self.staged_other += 1,
            _ => {}
        }
    }
}

/// Whether `x` and `y` agree on every task live in b, o and t.
fn judged_eq(x: &Hier, y: &Hier, t: &Tri) -> bool {
    (0..x.len()).all(|i| t.b[i].is_none() || t.o[i].is_none() || t.t[i].is_none() || x[i] == y[i])
}

/// One variant's counts over a set of histories.
#[derive(Clone, Debug, Default)]
pub(crate) struct Abs {
    pub histories: usize,
    pub panics: usize,
    pub with_fault: usize,
    pub secs: f64,
    pub derived_backstop: usize,
    pub total: KStats,
    pub by_kind: BTreeMap<Kind, KStats>,
}

impl Abs {
    fn add(&mut self, l: &Log) {
        self.histories += 1;
        self.panics += l.panic.is_some() as usize;
        self.with_fault += (l.faults() > 0) as usize;
        self.secs += l.secs;
        self.derived_backstop += l.derived_backstop;
        for e in &l.entries {
            self.total.add(e);
            if e.kind != Kind::Edit {
                self.by_kind.entry(e.kind).or_default().add(e);
            }
        }
    }
}

/// A variant against one reference over a set of histories.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rel {
    /// (divergence, variant's fault there, reference's fault there) → histories.
    pub div: BTreeMap<(Div, bool, bool), usize>,
    /// Histories where the variant has fewer, as many and more oracle faults than the reference.
    pub fewer: usize,
    pub equal: usize,
    pub more: usize,
}

impl Rel {
    fn add(&mut self, c: &Cmp) {
        *self.div.entry((c.div, c.a_fault, c.r_fault)).or_default() += 1;
        match c.faults.0.cmp(&c.faults.1) {
            std::cmp::Ordering::Less => self.fewer += 1,
            std::cmp::Ordering::Equal => self.equal += 1,
            std::cmp::Ordering::Greater => self.more += 1,
        }
    }

    fn of(&self, d: Div) -> (usize, [usize; 4]) {
        let g = |a: bool, r: bool| self.div.get(&(d, a, r)).copied().unwrap_or(0);
        let parts = [
            g(true, false),
            g(false, true),
            g(true, true),
            g(false, false),
        ];
        (parts.iter().sum(), parts)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Text.

fn hv_text(v: &HV) -> String {
    match v {
        None => "dead".into(),
        Some((p, o)) => format!(
            "{}{}",
            p.map_or("root".to_string(), |p| format!("<-#{p}")),
            o.as_ref().map_or(String::new(), |o| format!("[{o}]"))
        ),
    }
}

pub(crate) fn hier_text(h: &Hier) -> String {
    h.iter()
        .enumerate()
        .map(|(i, v)| format!("#{}{}", i + 1, hv_text(v)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn got_text(g: &Got) -> String {
    match g {
        Got::Ok => "ok".into(),
        Got::Landed => "landed".into(),
        Got::UpToDate => "up-to-date".into(),
        Got::Staged(k) => format!("staged[{}]", k.join(", ")),
        Got::Refused(c) => format!("refused({c})"),
        Got::Skipped => "skipped".into(),
    }
}

fn verdict_text(v: &Verdict) -> String {
    match v {
        Verdict::LandedOk { took_o, took_t } => {
            if took_o + took_t == 0 {
                "ok".into()
            } else {
                format!("ok, two-sided took o {took_o} t {took_t}")
            }
        }
        Verdict::LandedWrong {
            one_sided,
            untouched,
            both_same,
            neither,
            order_only,
        } => format!(
            "WRONG landing: one-sided {one_sided}, untouched {untouched}, both-same {both_same}, neither {neither}{}",
            if *order_only { ", order only" } else { "" }
        ),
        Verdict::Avoidable {
            on_determined,
            cand_wrong,
            stuck,
        } => format!(
            "AVOIDABLE staging{}{}{}",
            if *on_determined {
                ", on a determined key"
            } else {
                ""
            },
            if *cand_wrong {
                ", candidate moves a determined key"
            } else {
                ""
            },
            if *stuck {
                ", ours/theirs cannot resolve it"
            } else {
                ""
            }
        ),
        Verdict::Genuine { cand_wrong, stuck } => format!(
            "genuine staging{}{}",
            if *cand_wrong {
                ", candidate moves a determined key"
            } else {
                ""
            },
            if *stuck {
                ", ours/theirs cannot resolve it"
            } else {
                ""
            }
        ),
        Verdict::StagedOther => "staged (no HierarchyCycle)".into(),
        Verdict::Refused => "refused".into(),
        Verdict::UpToDate => "up to date".into(),
    }
}

/// The oracle's expectation on (b, o, t), as text.
pub(crate) fn expectation_text(t: &Tri) -> String {
    let or = oracle(&t.b, &t.o, &t.t);
    let mut det = Vec::new();
    let mut two = Vec::new();
    for i in 0..t.b.len() {
        match or.class[i] {
            Class::NotJudged => det.push(format!("#{} not judged", i + 1)),
            Class::BothDiff => two.push(format!(
                "#{} o {} | t {}",
                i + 1,
                hv_text(&t.o[i]),
                hv_text(&t.t[i])
            )),
            c => det.push(format!(
                "#{} {} ({})",
                i + 1,
                hv_text(or.expected(i, &t.b, &t.o, &t.t).expect("determined")),
                match c {
                    Class::Untouched => "untouched",
                    Class::OnlyO => "o only",
                    Class::OnlyT => "t only",
                    _ => "both, same",
                }
            )),
        }
    }
    format!(
        "{}; determined: {}{}",
        if or.forest {
            "a forest keeps every constraint: must land"
        } else {
            "no forest keeps the constraints: must stage"
        },
        det.join(", "),
        if two.is_empty() {
            String::new()
        } else {
            format!("; two-sided (either; MR-040 picks): {}", two.join(", "))
        }
    )
}

fn entry_text(e: &Entry) -> String {
    let mut s = format!("{}: {}", e.kind.name(), got_text(&e.got));
    match e.settled {
        Some(true) => s.push_str(" -> resolved, landed"),
        Some(false) => s.push_str(" -> resolution aborted"),
        None => {}
    }
    if let Some(v) = &e.verdict {
        let _ = write!(s, " => {}", verdict_text(v));
    }
    if let (Some(v), Some(w)) = (&e.verdict, &e.verdict_ch)
        && std::mem::discriminant(v) != std::mem::discriminant(w)
    {
        let _ = write!(s, " [changeset-touch reading: {}]", verdict_text(w));
    }
    if e.backstop > 0 {
        let _ = write!(s, " (backstop {})", e.backstop);
    }
    if e.repairs > 0 {
        let _ = write!(s, " (repairs {})", e.repairs);
    }
    s
}

fn tri_text(t: &Tri) -> String {
    format!(
        "b {} | o {} | t {} | result {}{}{}",
        hier_text(&t.b),
        hier_text(&t.o),
        hier_text(&t.t),
        hier_text(&t.r),
        if t.virtual_base {
            " | virtual base"
        } else {
            ""
        },
        if t.one_sided { " | one-sided" } else { "" }
    )
}

/// A verbose trace of one history's log.
pub(crate) fn trace_text(h: &Hist, l: &Log) -> String {
    let mut s = String::new();
    if let Some(p) = &l.panic {
        let _ = writeln!(s, "  PANIC {p}");
    }
    for (i, ev) in h.ops.iter().enumerate() {
        let _ = writeln!(s, "  [{i}] {ev:?}");
        for e in l
            .entries
            .iter()
            .filter(|e| e.op == i && e.kind != Kind::Edit)
        {
            let _ = writeln!(s, "      {}", entry_text(e));
            if let Some(t) = &e.tri {
                let _ = writeln!(s, "        {}", tri_text(t));
                let _ = writeln!(s, "        oracle: {}", expectation_text(t));
            }
        }
        if let Some(e) = l.entries.iter().find(|e| e.op == i && e.kind == Kind::Edit)
            && e.got != Got::Ok
        {
            let _ = writeln!(s, "      edit {}", got_text(&e.got));
        }
    }
    if let Some(last) = l.snaps.last() {
        for (b, hh) in last {
            let _ = writeln!(s, "  end {b}: {}", hier_text(hh));
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------------------------------
// Generators.

/// A random-history generator. All histories have tasks #1 to #4, created at the root on `main`, with every lane
/// forked at that commit; a move puts #n under one of the other three (4 times in 5) or at the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Gen {
    /// The committed lockstep search's alphabet (`suite::kleppmann`): `main` and two lanes; moves, priority edits,
    /// syncs, cross-lane merges, merges into `main`; 1 to 23 events.
    Narrow,
    /// Narrow, plus moves at a position, order-only moves (a node reordered under its current parent) and deletes
    /// (with and without `POLICY REPARENT`); 1 to 23 events.
    Lanes,
    /// Three lanes; Lanes' events, cross-lane merges between any two lanes, criss-cross merges, cherry-picks and reverts
    /// of a random earlier commit; 1 to 40 events (`kwide.rs` of the wave 3d verifiers, widened).
    Wide,
    /// Lanes' events plus `merge --base C` of a random earlier commit C, between any two branches (S-4); 1 to 23 events.
    Based,
}

impl Gen {
    pub(crate) const ALL: [Gen; 4] = [Gen::Narrow, Gen::Lanes, Gen::Wide, Gen::Based];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Gen::Narrow => "narrow",
            Gen::Lanes => "lanes",
            Gen::Wide => "wide",
            Gen::Based => "based",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Gen> {
        Gen::ALL
            .into_iter()
            .find(|g| g.name().eq_ignore_ascii_case(s.trim()))
    }

    fn lanes(self) -> Vec<&'static str> {
        match self {
            Gen::Wide => vec![NAMES[1], NAMES[2], NAMES[3]],
            _ => vec![NAMES[1], NAMES[2]],
        }
    }
}

/// The events of generator `g`.
fn strategy(g: Gen) -> proptest::strategy::BoxedStrategy<Vec<Ev>> {
    use proptest::prelude::*;
    let nb: usize = if g == Gen::Wide { 4 } else { 3 };
    let under = |n: u32, d: Option<u32>| d.map(|d| (n - 1 + d) % 4 + 1);
    let target = || prop_oneof![4 => (1u32..=3).prop_map(Some), 1 => Just(None)];
    let take = || any::<bool>().prop_map(|o| if o { T::Ours } else { T::Theirs });
    let pos = || prop_oneof![Just(Pos::First), Just(Pos::Last)];
    let prio = (0..nb, 1u32..=4, 0i64..4).prop_map(|(b, n, k)| Ev::Prio(NAMES[b], n, k));
    let sync = (1..nb, take()).prop_map(|(l, t)| Ev::Sync(NAMES[l], t));
    let into_main = (1..nb, take()).prop_map(|(l, t)| Ev::Merge(NAMES[l], "main", t));
    let lanes = nb - 1;
    // Two different lanes.
    let pair = move || (1..nb, 1..lanes).prop_map(move |(a, d)| (a, (a - 1 + d) % lanes + 1));
    let cross = (pair(), take()).prop_map(|((a, d), t)| Ev::Merge(NAMES[a], NAMES[d], t));
    if g == Gen::Narrow {
        let mv =
            (0..nb, 1u32..=4, target()).prop_map(move |(b, n, d)| Ev::Mv(NAMES[b], n, under(n, d)));
        let op = prop_oneof![8 => mv, 1 => prio, 2 => sync, 2 => cross, 1 => into_main];
        return proptest::collection::vec(op, 1..24).boxed();
    }
    let mv = (
        0..nb,
        1u32..=4,
        target(),
        prop_oneof![3 => Just(None), 1 => pos().prop_map(Some)],
    )
        .prop_map(move |(b, n, d, p)| match p {
            None => Ev::Mv(NAMES[b], n, under(n, d)),
            Some(p) => Ev::MvAt(NAMES[b], n, under(n, d), p),
        });
    let reorder = (0..nb, 1u32..=4, pos()).prop_map(|(b, n, p)| Ev::Reorder(NAMES[b], n, p));
    let del = (0..nb, 1u32..=4, any::<bool>()).prop_map(|(b, n, r)| Ev::Del(NAMES[b], n, r));
    match g {
        Gen::Lanes => {
            let op = prop_oneof![
                8 => mv, 2 => reorder, 1 => prio, 1 => del, 2 => sync, 2 => cross, 1 => into_main
            ];
            proptest::collection::vec(op, 1..24).boxed()
        }
        Gen::Based => {
            // Any two different branches, `main` included.
            let any_pair = (0..nb, 1..nb).prop_map(move |(a, d)| (a, (a + d) % nb));
            let based = (any_pair, 0usize..64, take())
                .prop_map(|((a, d), k, t)| Ev::Based(NAMES[a], NAMES[d], CRef::Pool(k), t));
            let op = prop_oneof![
                8 => mv, 2 => reorder, 1 => prio, 1 => del, 2 => sync, 2 => cross, 1 => into_main, 2 => based
            ];
            proptest::collection::vec(op, 1..24).boxed()
        }
        _ => {
            let criss = (pair(), take(), take())
                .prop_map(|((a, d), t1, t2)| Ev::Criss(NAMES[a], NAMES[d], t1, t2));
            let revert = (0..nb, 0usize..64, take())
                .prop_map(|(b, k, t)| Ev::Revert(CRef::Pool(k), NAMES[b], t));
            let pick = (0..nb, 0usize..64, take())
                .prop_map(|(b, k, t)| Ev::Pick(CRef::Pool(k), NAMES[b], t));
            let op = prop_oneof![
                8 => mv, 2 => reorder, 1 => prio, 1 => del, 2 => sync, 3 => cross, 1 => into_main, 1 => criss,
                1 => revert, 1 => pick
            ];
            proptest::collection::vec(op, 1..41).boxed()
        }
    }
}

/// `n` histories of generator `g` with seed `seed`: a fixed ChaCha seed per (generator, seed), as the crate's other
/// property tests fix theirs, so every run of a tier sees the same histories.
pub(crate) fn histories(g: Gen, seed: u8, n: usize) -> Vec<Hist> {
    use proptest::strategy::{Strategy, ValueTree};
    let mut sb = *b"moirai-model/rs007/evaluate-seed";
    sb[31] ^= seed;
    sb[30] ^= 1 + g as u8;
    let mut runner = proptest::test_runner::TestRunner::new_with_rng(
        proptest::test_runner::Config::default(),
        proptest::test_runner::TestRng::from_seed(proptest::test_runner::RngAlgorithm::ChaCha, &sb),
    );
    let strat = strategy(g);
    (0..n)
        .map(|_| Hist {
            tasks: 4,
            lanes: g.lanes(),
            ops: strat.new_tree(&mut runner).expect("a history").current(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// The corpus.

/// A named history of the corpus, from the findings that RS-007's history of fixes left (open point 35, wave 3c's
/// arbiter, wave 3d's verifiers). Its expectation is the oracle's, computed on each variant's own states; the cases a
/// rule reading decides are `readings`.
#[derive(Clone, Debug)]
pub(crate) struct Case {
    /// The id.
    pub id: &'static str,
    /// The finding or example it comes from.
    pub source: &'static str,
    /// What the history is.
    pub note: &'static str,
    /// The history.
    pub hist: Hist,
    /// The events whose merges the corpus reports (the last event when empty).
    pub focus: Vec<usize>,
    /// CONTESTED: the readings of MR-040 that decide the focus merge differently, each with the parent it gives
    /// task `.1` (`.2`; `None`: the root); the oracle allows both (a key touched on both sides).
    pub readings: Vec<(&'static str, u32, Option<u32>)>,
}

const M: &str = "main";
const X: &str = "lane/x";
const Y: &str = "lane/y";
const LA: &str = "lane/a";
const LB: &str = "lane/b";
const Z: &str = "lane/z";

fn case(
    id: &'static str,
    source: &'static str,
    note: &'static str,
    tasks: u32,
    lanes: &[&'static str],
    ops: Vec<Ev>,
    focus: &[usize],
) -> Case {
    Case {
        id,
        source,
        note,
        hist: Hist {
            tasks,
            lanes: lanes.to_vec(),
            ops,
        },
        focus: focus.to_vec(),
        readings: Vec::new(),
    }
}

/// The E6 history ([RULES/merge-table] open point 35 (v) E6), up to and including its cross-lane merge.
fn e6_ops() -> Vec<Ev> {
    use Ev::*;
    vec![
        Mv(Y, 2, Some(3)),
        Mv(X, 1, Some(3)),
        Mv(M, 4, Some(2)),
        Mv(X, 3, Some(4)),
        Mv(M, 2, Some(1)),
        Merge(X, Y, T::Ours),
        Sync(X, T::Ours),
        Merge(Y, X, T::Ours),
    ]
}

/// The corpus.
pub(crate) fn corpus() -> Vec<Case> {
    use Ev::*;
    use T::{Abort, Ours, Theirs};
    let mut v = vec![
        case(
            "E1",
            "OP35 (v) E1; OQ-A-11 11.1 (A)",
            "lane/x: #2 under #3, #1 under #2, #2 back at the root; main: #3 under #1; merge lane/x --into main",
            3,
            &[X],
            vec![
                Mv(X, 2, Some(3)),
                Mv(X, 1, Some(2)),
                Mv(X, 2, None),
                Mv(M, 3, Some(1)),
                Merge(X, M, Ours),
            ],
            &[],
        ),
        case(
            "E2",
            "OP35 (v) E2; OQ-A-11 11.1 (A)",
            "lane/x: #2 under #1; main: #1 under #2; sync (stages #1, ours); lane/x: #1 under #3; merge into main",
            3,
            &[X],
            vec![
                Mv(X, 2, Some(1)),
                Mv(M, 1, Some(2)),
                Sync(X, Ours),
                Mv(X, 1, Some(3)),
                Merge(X, M, Ours),
            ],
            &[2, 4],
        ),
        case(
            "E3",
            "OP35 (v) E3; OQ-A-11 11.1 (B)",
            "E2's history to lane/x's #1 under #3; main sets #3.priority; sync again",
            3,
            &[X],
            vec![
                Mv(X, 2, Some(1)),
                Mv(M, 1, Some(2)),
                Sync(X, Ours),
                Mv(X, 1, Some(3)),
                Prio(M, 3, 1),
                Sync(X, Ours),
            ],
            &[],
        ),
        case(
            "E4",
            "OP35 (v) E4; OQ-A-11 11.1 (A)",
            "lane/b: #1 under #2; lane/a: #2 under #1, #1 under #3; merge lane/b --into lane/a (ours); merge lane/a into main",
            3,
            &[LA, LB],
            vec![
                Mv(LB, 1, Some(2)),
                Mv(LA, 2, Some(1)),
                Mv(LA, 1, Some(3)),
                Merge(LB, LA, Ours),
                Merge(LA, M, Ours),
            ],
            &[3, 4],
        ),
        case(
            "E5a",
            "OP35 (v) E5 first shape; OQ-A-12 (c)",
            "E4's history to its cross merge (ours); main sets #3.priority; sync lane/a",
            3,
            &[LA, LB],
            vec![
                Mv(LB, 1, Some(2)),
                Mv(LA, 2, Some(1)),
                Mv(LA, 1, Some(3)),
                Merge(LB, LA, Ours),
                Prio(M, 3, 1),
                Sync(LA, Ours),
            ],
            &[],
        ),
        case(
            "E5b",
            "OP35 (v) E5 second shape; W3C-ARB-2; OQ-A-12 (c)",
            "main: #2 under #1; lane/x: #1 under #2, #2 under #3; sync (stages #1, ours); then main edits a priority and lane/x syncs, three times",
            3,
            &[X],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 1, Some(2)),
                Mv(X, 2, Some(3)),
                Sync(X, Ours),
                Prio(M, 3, 1),
                Sync(X, Ours),
                Prio(M, 3, 2),
                Sync(X, Ours),
                Prio(M, 3, 3),
                Sync(X, Ours),
            ],
            &[3, 5, 7, 9],
        ),
        case(
            "E6",
            "OP35 (v) E6; W3C-ARB-1; OQ-A-12 (b)",
            "lane/y #2 under #3; lane/x #1 under #3; main #4 under #2; lane/x #3 under #4; main #2 under #1; merge x->y; sync x (stages #2, ours); merge y->x",
            4,
            &[X, Y],
            e6_ops(),
            &[],
        ),
    ];
    let mut crit3 = e6_ops();
    for r in 1..=3 {
        crit3.extend([Prio(M, 4, r), Sync(X, Ours), Sync(Y, Ours)]);
    }
    crit3.extend([Merge(X, M, Ours), Merge(Y, M, Ours)]);
    v.push(case(
        "CRIT-3",
        "CRIT-3; ADV-B-6",
        "E6, then three rounds of (main sets #4.priority; sync lane/x; sync lane/y), then both lanes into main",
        4,
        &[X, Y],
        crit3,
        &[9, 10, 12, 13, 15, 16, 17, 18],
    ));
    v.extend([
        case(
            "CRIT-2-sync",
            "CRIT-2 (critic_away_and_back_on_a_sync)",
            "lane/x: #2 under #1; main: #1 under #2; lane/x: #2 back at the root; sync lane/x (o = b)",
            4,
            &[X, Y],
            vec![Mv(X, 2, Some(1)), Mv(M, 1, Some(2)), Mv(X, 2, None), Sync(X, Ours)],
            &[],
        ),
        case(
            "CRIT-2-cross",
            "CRIT-2 (critic_min)",
            "lane/y: #2 under #1; lane/x: #1 under #2; lane/y: #2 back at the root; merge lane/x --into lane/y",
            4,
            &[X, Y],
            vec![Mv(Y, 2, Some(1)), Mv(X, 1, Some(2)), Mv(Y, 2, None), Merge(X, Y, Ours)],
            &[],
        ),
        case(
            "W3D-REV-1",
            "W3D-REV-1 (seed 23); CRIT-1",
            "a kept key's transient value during the replay undoes a move only main made",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(3)),
                Mv(Y, 1, Some(2)),
                Mv(M, 3, Some(1)),
                Sync(Y, Ours),
                Mv(M, 2, Some(1)),
                Sync(X, Theirs),
                Mv(Y, 1, None),
                Sync(Y, Theirs),
            ],
            &[],
        ),
        case(
            "S-1A",
            "S-1 shape A",
            "(c) re-asserts a resolution-kept older move: silent wrong value",
            4,
            &[X, Y],
            vec![
                Mv(M, 4, Some(1)),
                Mv(X, 1, Some(4)),
                Merge(X, Y, Theirs),
                Mv(M, 4, None),
                Mv(X, 1, None),
                Sync(Y, Ours),
                Merge(Y, X, Ours),
            ],
            &[],
        ),
        case(
            "S-1B",
            "S-1 shape B",
            "(c) re-asserts a resolution-kept older move: silent wrong value",
            4,
            &[X, Y],
            vec![
                Mv(M, 3, Some(2)),
                Mv(Y, 2, Some(3)),
                Merge(Y, X, Theirs),
                Mv(X, 2, None),
                Mv(M, 3, Some(1)),
                Sync(Y, Ours),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
        case(
            "S-1C",
            "S-1 shape C",
            "(c)'s re-asserted move blocks a later move: a staging where wave 3c and the replay from B land",
            4,
            &[X, Y],
            vec![
                Mv(M, 4, Some(1)),
                Mv(X, 1, Some(4)),
                Mv(X, 4, Some(3)),
                Merge(X, Y, Ours),
                Mv(Y, 1, None),
                Sync(X, Ours),
                Mv(Y, 4, Some(1)),
                Merge(X, Y, Theirs),
            ],
            &[],
        ),
        case(
            "ADV-B-7",
            "ADV-B-7",
            "(c): a staging where both baselines land",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(4)),
                Mv(X, 1, Some(2)),
                Mv(Y, 1, Some(4)),
                Mv(X, 4, Some(1)),
                Mv(M, 2, Some(3)),
                Merge(X, Y, Ours),
                Sync(X, Ours),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
        case(
            "ADV-B-8",
            "ADV-B-8",
            "(c): a landing that drops lane/y's resolution",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(4)),
                Mv(M, 1, Some(2)),
                Mv(Y, 1, Some(4)),
                Mv(X, 4, Some(1)),
                Mv(M, 2, Some(3)),
                Merge(X, Y, Ours),
                Sync(X, Ours),
                Merge(X, Y, Theirs),
            ],
            &[],
        ),
        case(
            "ADV-B-2",
            "ADV-B-2",
            "the backstop resets a key both sides moved alike (5 tasks)",
            5,
            &[X, Y],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 3, Some(4)),
                Mv(X, 1, Some(3)),
                Mv(M, 4, Some(5)),
                Mv(Y, 4, Some(2)),
                Merge(Y, X, Ours),
                Sync(Y, Ours),
                Mv(Y, 3, Some(4)),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
        case(
            "ADV-B-2-base",
            "ADV-B-2 (only --take base lands)",
            "ADV-B-2's history, resolved with --take base",
            5,
            &[X, Y],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 3, Some(4)),
                Mv(X, 1, Some(3)),
                Mv(M, 4, Some(5)),
                Mv(Y, 4, Some(2)),
                Merge(Y, X, Ours),
                Sync(Y, Ours),
                Mv(Y, 3, Some(4)),
                Merge(X, Y, T::Base),
            ],
            &[],
        ),
        case(
            "ADV-B-3",
            "ADV-B-3",
            "backstop cascade: a reset closes a new cycle (5 tasks, #2 under #5 at the fork)",
            5,
            &[],
            vec![
                Mv(M, 2, Some(5)),
                Fork(X, M),
                Fork(Y, M),
                Mv(M, 2, Some(1)),
                Mv(X, 1, Some(3)),
                Mv(M, 3, Some(4)),
                Mv(Y, 3, Some(2)),
                Merge(Y, X, Ours),
                Sync(Y, Ours),
                Mv(Y, 5, Some(2)),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
        case(
            "S-2",
            "S-2; ADV-C-1; W3D-REV-2 (backstop at the nightly tier)",
            "main #2 under #1; lane/x #1 under #3; lane/y #3 under #2; merge y->x; sync y; merge y->x",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 1, Some(3)),
                Mv(Y, 3, Some(2)),
                Merge(Y, X, Ours),
                Sync(Y, Ours),
                Merge(Y, X, Theirs),
            ],
            &[],
        ),
        case(
            "ADV-B-1",
            "ADV-B-1; W3D-REV-2 (backstop, shrunk at the nightly tier)",
            "main #2 under #1; lane/x #1 under #3; main #3 under #4; lane/y #3 under #2; merge y->x; sync y; merge x->y",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 1, Some(3)),
                Mv(M, 3, Some(4)),
                Mv(Y, 3, Some(2)),
                Merge(Y, X, Ours),
                Sync(Y, Ours),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
        case(
            "W3D-REV-3",
            "W3D-REV-3",
            "the backstop stages a key whose ours resolution re-stages",
            4,
            &[X, Y],
            vec![
                Mv(Y, 3, Some(1)),
                Mv(X, 2, Some(3)),
                Mv(M, 1, Some(2)),
                Sync(X, Ours),
                Sync(Y, Ours),
                Merge(X, Y, Ours),
            ],
            &[],
        ),
    ]);
    let mut c = case(
        "ADV-C-6-yx",
        "W3D-REV-5; ADV-C-6; CRIT-4 (CONTESTED)",
        "main #2 under #1; lane/x #1 under #2, #2 under #3; lane/y #1 under #4; sync x (stages #1, ours); merge y->x",
        4,
        &[X, Y],
        vec![
            Mv(M, 2, Some(1)),
            Mv(X, 1, Some(2)),
            Mv(X, 2, Some(3)),
            Mv(Y, 1, Some(4)),
            Sync(X, Ours),
            Merge(Y, X, Ours),
        ],
        &[],
    );
    c.readings = vec![
        (
            "a resolution is a move at the resolving sync's time (OQ-A-12 (c))",
            1,
            Some(2),
        ),
        (
            "MR-040 by the original moves' times (lane/y's move is later than lane/x's)",
            1,
            Some(4),
        ),
    ];
    v.push(c.clone());
    c.id = "ADV-C-6-xy";
    c.note = "the same history; merge x->y";
    c.hist.ops[5] = Merge(X, Y, Ours);
    v.push(c);
    let mut c = case(
        "W3D-REV-5",
        "W3D-REV-5 (seed 11) (CONTESTED)",
        "a sync resolved to ours keeps lane/x's #4 under #1 (X2); lane/y moved #4 under #3 (Y1) after X2 and before the sync",
        4,
        &[X, Y],
        vec![
            Mv(M, 2, Some(4)),
            Mv(X, 1, Some(2)),
            Mv(M, 2, Some(1)),
            Mv(X, 4, Some(1)),
            Mv(M, 1, Some(2)),
            Mv(X, 1, Some(2)),
            Mv(Y, 4, Some(3)),
            Sync(X, Ours),
            Merge(X, Y, Ours),
        ],
        &[],
    );
    c.readings = vec![
        (
            "a resolution is a move at the resolving sync's time (OQ-A-12 (c))",
            4,
            Some(1),
        ),
        (
            "MR-040 by the original moves' times (Y1 is later than X2)",
            4,
            Some(3),
        ),
    ];
    v.push(c);
    v.extend([
        case(
            "S-4a",
            "S-4 (w_base_on_src_tip)",
            "main, in one commit s2, puts #4 under #3 and sets #4.priority; merge main --into lane/y --base s2",
            4,
            &[X, Y],
            vec![
                Tx(M, vec![It::Mv(4, Some(3)), It::Prio(4, 0)]),
                Mark("s2", M),
                Based(M, Y, CRef::Label("s2"), Ours),
            ],
            &[],
        ),
        case(
            "S-4b",
            "S-4 (base_worse)",
            "lane/x #1 under #2 (s2); lane/y #4 under #2, #2 under #1; merge lane/x --into lane/y --base s2 (b = t)",
            4,
            &[X, Y],
            vec![
                Mv(X, 1, Some(2)),
                Mark("s2", X),
                Mv(Y, 4, Some(2)),
                Mv(Y, 2, Some(1)),
                Based(X, Y, CRef::Label("s2"), Theirs),
            ],
            &[],
        ),
        case(
            "ADV-C-3",
            "ADV-C-3",
            "main #2 under #1; lane/x #1 under #2 (C), #2 under #3; sync (stages #1, ours); revert C onto lane/x",
            4,
            &[X, Y],
            vec![
                Mv(M, 2, Some(1)),
                Mv(X, 1, Some(2)),
                Mark("C", X),
                Mv(X, 2, Some(3)),
                Sync(X, Ours),
                Revert(CRef::Label("C"), X, Ours),
            ],
            &[],
        ),
        case(
            "X1",
            "X1-style genuine cycle",
            "lane/x: #1 under #2; lane/y: #2 under #1; merge lane/y --into lane/x: must stage",
            4,
            &[X, Y],
            vec![Mv(X, 1, Some(2)), Mv(Y, 2, Some(1)), Merge(Y, X, Abort)],
            &[],
        ),
        case(
            "X1-sync",
            "X1-style genuine cycle on a sync",
            "lane/x: #1 under #2; main: #2 under #1; sync lane/x: must stage",
            3,
            &[X],
            vec![Mv(X, 1, Some(2)), Mv(M, 2, Some(1)), Sync(X, Abort)],
            &[],
        ),
        case(
            "SWAP",
            "R29 (a_swap_in_one_commit_merges_clean)",
            "main: #2 under #1; fork; lane/x swaps parent and child in one transaction; main edits; merge into main: must land",
            3,
            &[],
            vec![
                Mv(M, 2, Some(1)),
                Fork(X, M),
                Tx(X, vec![It::Mv(2, None), It::Mv(1, Some(2))]),
                Prio(M, 3, 1),
                Merge(X, M, Ours),
            ],
            &[],
        ),
        case(
            "RESTRUCT-sync",
            "OP15 (a_restructure_over_three_commits_syncs_and_merges)",
            "main: #2 under #1; fork; lane/x: #2 to the root, #1 under #2, #2 under #3; main edits; sync, then merge into main: must land",
            4,
            &[],
            vec![
                Mv(M, 2, Some(1)),
                Fork(X, M),
                Mv(X, 2, None),
                Mv(X, 1, Some(2)),
                Mv(X, 2, Some(3)),
                Prio(M, 4, 1),
                Sync(X, Ours),
                Merge(X, M, Ours),
            ],
            &[6, 7],
        ),
        case(
            "RESTRUCT-merge",
            "OP15 (a_restructure_over_three_commits_syncs_and_merges)",
            "the same, merged into main directly (step 0's sync first): must land",
            4,
            &[],
            vec![
                Mv(M, 2, Some(1)),
                Fork(X, M),
                Mv(X, 2, None),
                Mv(X, 1, Some(2)),
                Mv(X, 2, Some(3)),
                Prio(M, 4, 1),
                Merge(X, M, Ours),
            ],
            &[],
        ),
        case(
            "OP35-vi",
            "OP35 (vi); OQ-A-11 11.2",
            "#1 under #3 at the fork; ours: #3 under #2; theirs, later, in one transaction: #1 reordered under #3 and #2 under #1; merge theirs into ours: must stage (#1 keeps theirs' order)",
            4,
            &[],
            vec![
                Mv(M, 1, Some(3)),
                Fork(X, M),
                Fork(Y, M),
                Mv(X, 3, Some(2)),
                Tx(Y, vec![It::MvAt(1, Some(3), Pos::First), It::Mv(2, Some(1))]),
                Merge(Y, X, Abort),
            ],
            &[],
        ),
        case(
            "OP35-vi-land",
            "OP35 (vi), order-only move that must land",
            "#1, #2 under #3 at the fork; lane/x reorders #1 first under #3; main: #2 to the root, #3 under #2; sync lane/x",
            4,
            &[],
            vec![
                Mv(M, 1, Some(3)),
                Mv(M, 2, Some(3)),
                Fork(X, M),
                Reorder(X, 1, Pos::First),
                Mv(M, 2, None),
                Mv(M, 3, Some(2)),
                Sync(X, Ours),
            ],
            &[],
        ),
        case(
            "OP35-i",
            "OP35 (i) (a_syncs_step_reasserts_the_key_its_resolution_kept_against_main)",
            "lane/x #2 under #1; lane/y #2 under #4; main #1 under #2; sync x (stages #1, ours); merge x->y",
            4,
            &[X, Y],
            vec![Mv(X, 2, Some(1)), Mv(Y, 2, Some(4)), Mv(M, 1, Some(2)), Sync(X, Ours), Merge(X, Y, Ours)],
            &[3, 4],
        ),
        case(
            "OP35-ii",
            "OP35 (ii) (a_cherry_pick_of_a_commit_with_no_move_keeps_dsts_hierarchy)",
            "lane/x #1 under #2, then C sets #4.priority; main #2 under #1, #1 under #3; cherry-pick C onto main: lands main's hierarchy",
            4,
            &[X],
            vec![
                Mv(X, 1, Some(2)),
                Prio(X, 4, 0),
                Mark("C", X),
                Mv(M, 2, Some(1)),
                Mv(M, 1, Some(3)),
                Pick(CRef::Label("C"), M, Ours),
            ],
            &[],
        ),
        case(
            "OP35-iii",
            "OP35 (iii)",
            "ours, one commit: #3 under #2, #1 under #3; theirs, later, one commit: #1 under #3, #2 under #1; merge theirs into ours: must stage",
            4,
            &[X, Y],
            vec![
                Tx(X, vec![It::Mv(3, Some(2)), It::Mv(1, Some(3))]),
                Tx(Y, vec![It::Mv(1, Some(3)), It::Mv(2, Some(1))]),
                Merge(Y, X, Abort),
            ],
            &[],
        ),
        case(
            "OP35-iii-b",
            "OP35 (iii), second part",
            "the same, then ours puts #2 under #4 in a still later commit: must land",
            4,
            &[X, Y],
            vec![
                Tx(X, vec![It::Mv(3, Some(2)), It::Mv(1, Some(3))]),
                Tx(Y, vec![It::Mv(1, Some(3)), It::Mv(2, Some(1))]),
                Mv(X, 2, Some(4)),
                Merge(Y, X, Ours),
            ],
            &[],
        ),
        case(
            "RETIME-MAIN",
            "this harness (narrow:1:38's class): a merge into main re-times the merged lane's move",
            "lane/y #2 under #3 (Y1); merge y->x; lane/x #2 under #4 (X); merge lane/y --into main (M, after X); sync lane/y; merge y->x: only lane/x changed #2 since the base",
            4,
            &[X, Y],
            vec![
                Mv(Y, 2, Some(3)),
                Merge(Y, X, Ours),
                Mv(X, 2, Some(4)),
                Merge(Y, M, Ours),
                Sync(Y, Ours),
                Merge(Y, X, Ours),
            ],
            &[],
        ),
        case(
            "REVERT-AFTER-MERGE",
            "this harness (wide:1:42's class); ADV-C-3's mechanism without a resolution",
            "lane/z #2 under #3 (C); merge lane/z --into lane/y (one-sided); revert C onto lane/y: the merge commit counts as a later move of #2",
            4,
            &[Y, Z],
            vec![
                Mv(Z, 2, Some(3)),
                Mark("C", Z),
                Merge(Z, Y, Ours),
                Revert(CRef::Label("C"), Y, Ours),
            ],
            &[],
        ),
    ]);
    v
}

/// The corpus case `id`.
pub(crate) fn corpus_case(id: &str) -> Option<Case> {
    corpus().into_iter().find(|c| c.id.eq_ignore_ascii_case(id))
}

/// The focus entries of a case's log.
fn focus_entries<'a>(c: &Case, l: &'a Log) -> Vec<&'a Entry> {
    let focus: Vec<usize> = if c.focus.is_empty() {
        vec![c.hist.ops.len() - 1]
    } else {
        c.focus.clone()
    };
    l.entries
        .iter()
        .filter(|e| e.kind != Kind::Edit && focus.contains(&e.op))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Cost.

/// The long-lane cost scenario (ADV-C-4's `c1`, `wave-3c-arbiter.md` W3C-ARB-10) under one variant.
#[derive(Clone, Debug)]
pub(crate) struct CostRow {
    pub total_s: f64,
    pub commits: usize,
    pub syncs: usize,
    pub staged: usize,
    pub aborted: usize,
    pub last10_ms: f64,
    pub max_ms: f64,
    pub cold_s: f64,
    pub next_ms: f64,
}

/// Six tasks, `main`, `lane/x` and `lane/y`; `n` rounds of: `main` moves a task under a lower-numbered one and edits a
/// priority, `lane/x` moves a task (mostly under a higher-numbered one, which closes cycles against `main`), `sync
/// lane/x` (timed; a staging resolved to ours, aborted if that does not land); every 25 rounds `lane/y` moves a task,
/// merges into `lane/x` (aborted when it stages) and syncs (aborted when it stages). Then, on a copy of the store with
/// an empty step-key memo, the step keys of `lane/x`'s tip are computed ("step keys from an empty memo"), and one more
/// edit on `main` and `sync lane/x` are timed.
pub(crate) fn cost(rule: Rule, n: usize) -> CostRow {
    under(rule, || {
        let h = Hist {
            tasks: 6,
            lanes: vec![X, Y],
            ops: Vec::new(),
        };
        let mut r = Run::new(&h, rule);
        let t0 = std::time::Instant::now();
        let mut sync_ms: Vec<f64> = Vec::new();
        let (mut staged, mut aborted) = (0, 0);
        let mut x: u64 = 12345;
        let mut rnd = |m: u32| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((x >> 33) % m as u64) as u32
        };
        let mv = |n: u32, p: Option<u32>| tx(vec![move_stmt(n, p, None)]);
        let sync = |l: &str| Cmd::Sync {
            lane: Some(l.into()),
            check: false,
        };
        let abort = |s: &mut S, src: &str, into: &str| {
            s.run(
                Cmd::MergeAbort {
                    src: Some(src.into()),
                    into: Some(into.into()),
                },
                fresh_on(into),
            );
        };
        for i in 0..n {
            let s = &mut r.s;
            let (a, b) = (rnd(6) + 1, rnd(6) + 1);
            let (lo, hi) = (a.min(b), a.max(b));
            if lo != hi {
                s.run(mv(hi, Some(lo)), fresh_on(M));
            }
            s.run(
                tx(vec![set(rnd(6) + 1, &[("priority", P::Int(i as i64 % 4))])]),
                fresh_on(M),
            );
            let (a, b) = (rnd(6) + 1, rnd(6) + 1);
            let (lo, hi) = (a.min(b), a.max(b));
            if lo != hi {
                let rr = if rnd(3) == 0 {
                    s.run(mv(hi, Some(lo)), fresh_on(X))
                } else {
                    s.run(mv(lo, Some(hi)), fresh_on(X))
                };
                if rr.outcome != Outcome::Ok {
                    s.run(mv(lo, None), fresh_on(X));
                }
            }
            let t = std::time::Instant::now();
            let rr = s.run(sync(X), fresh_on(X));
            if rr.outcome == Outcome::Staged {
                staged += 1;
                if let Data::Merge(d) = &rr.data {
                    let g = d.staging_ref.clone().unwrap_or_default();
                    for k in d
                        .violations
                        .iter()
                        .map(|v| v.key.clone())
                        .filter(|k| k != "-")
                    {
                        s.run(
                            tx(vec![Stmt::Resolve {
                                key: k,
                                take: Take::Ours,
                            }]),
                            fresh_on(&g),
                        );
                    }
                }
                let c = s.run(
                    Cmd::MergeContinue {
                        src: Some(M.into()),
                        into: Some(X.into()),
                    },
                    fresh_on(X),
                );
                if c.outcome != Outcome::Ok {
                    aborted += 1;
                    abort(s, M, X);
                }
            }
            sync_ms.push(t.elapsed().as_secs_f64() * 1e3);
            if i % 25 == 24 {
                let (a, b) = (rnd(6) + 1, rnd(6) + 1);
                if a != b {
                    s.run(mv(a, Some(b)), fresh_on(Y));
                }
                let rr = s.run(merge_cmd(Y, X, None), fresh_on(X));
                if rr.outcome == Outcome::Staged {
                    abort(s, Y, X);
                }
                s.run(sync(Y), fresh_on(Y));
                if s.st.dag.live("merge/lane/y/from/main").is_some() {
                    abort(s, M, Y);
                }
            }
        }
        let total_s = t0.elapsed().as_secs_f64();
        let k = sync_ms.len();
        let last = &sync_ms[k.saturating_sub(10)..];
        let last10_ms = last.iter().sum::<f64>() / last.len().max(1) as f64;
        let max_ms = sync_ms.iter().copied().fold(0.0, f64::max);
        let commits = r.s.st.dag.commits.len();
        // The same store with an empty step-key memo.
        let mut d = S { st: r.s.st.clone() };
        d.st.dag.step_memo.borrow_mut().clear();
        let t = std::time::Instant::now();
        let tipx =
            d.st.dag
                .live(X)
                .and_then(|r| r.tip)
                .expect("lane/x has a tip");
        let _ = d.st.dag.step_keys(tipx, &d.st.alloc);
        let cold_s = t.elapsed().as_secs_f64();
        d.run(tx(vec![set(1, &[("priority", P::Int(3))])]), fresh_on(M));
        let t = std::time::Instant::now();
        d.run(sync(X), fresh_on(X));
        let next_ms = t.elapsed().as_secs_f64() * 1e3;
        CostRow {
            total_s,
            commits,
            syncs: k,
            staged,
            aborted,
            last10_ms,
            max_ms,
            cold_s,
            next_ms,
        }
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// The report.

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| d.to_string())
}

/// The report's configuration, from the environment.
#[derive(Clone, Debug)]
pub(crate) struct Cfg {
    /// `MOIRAI_RS007_EVAL_RULE` (default `cand`): the variant under test.
    pub variant: Rule,
    /// `MOIRAI_RS007_EVAL_VS` (default `current,wave3c,fromb` less the variant; `none` for none): the references.
    pub refs: Vec<Rule>,
    /// `MOIRAI_RS007_EVAL_CASES` (default 200): histories per generator and seed.
    pub cases: usize,
    /// `MOIRAI_RS007_EVAL_SEEDS` (default `1,2`).
    pub seeds: Vec<u8>,
    /// `MOIRAI_RS007_EVAL_GENS` (default `narrow,lanes,wide,based`).
    pub gens: Vec<Gen>,
    /// `MOIRAI_RS007_EVAL_PARTS` (default `corpus,search,cost`).
    pub parts: BTreeSet<String>,
    /// `MOIRAI_RS007_EVAL_THREADS` (default 2).
    pub threads: usize,
    /// `MOIRAI_RS007_EVAL_EXAMPLES` (default 2): examples printed per category.
    pub examples: usize,
    /// `MOIRAI_RS007_EVAL_SHRINK` (default 0): examples shrunk per category (greedy, one event at a time).
    pub shrink: usize,
    /// `MOIRAI_RS007_EVAL_COST_N` (default `150,300`).
    pub cost_n: Vec<usize>,
}

impl Cfg {
    pub(crate) fn from_env() -> Cfg {
        let variant = Rule::parse(&env_or("MOIRAI_RS007_EVAL_RULE", "cand"))
            .expect("MOIRAI_RS007_EVAL_RULE: current, wave3c, fromb or cand");
        let vs = env_or("MOIRAI_RS007_EVAL_VS", "current,wave3c,fromb");
        let refs: Vec<Rule> = if vs.trim() == "none" {
            Vec::new()
        } else {
            vs.split(',')
                .map(|r| {
                    Rule::parse(r)
                        .expect("MOIRAI_RS007_EVAL_VS: a list of current, wave3c, fromb, cand")
                })
                .filter(|r| *r != variant)
                .collect()
        };
        let list = |k: &str, d: &str| -> Vec<String> {
            env_or(k, d)
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        };
        Cfg {
            variant,
            refs,
            cases: env_or("MOIRAI_RS007_EVAL_CASES", "200")
                .parse()
                .expect("MOIRAI_RS007_EVAL_CASES"),
            seeds: list("MOIRAI_RS007_EVAL_SEEDS", "1,2")
                .iter()
                .map(|x| x.parse().expect("MOIRAI_RS007_EVAL_SEEDS: numbers 0-255"))
                .collect(),
            gens: list("MOIRAI_RS007_EVAL_GENS", "narrow,lanes,wide,based")
                .iter()
                .map(|x| Gen::parse(x).expect("MOIRAI_RS007_EVAL_GENS: narrow, lanes, wide, based"))
                .collect(),
            parts: list("MOIRAI_RS007_EVAL_PARTS", "corpus,search,cost")
                .into_iter()
                .collect(),
            threads: env_or("MOIRAI_RS007_EVAL_THREADS", "2")
                .parse()
                .expect("MOIRAI_RS007_EVAL_THREADS"),
            examples: env_or("MOIRAI_RS007_EVAL_EXAMPLES", "2")
                .parse()
                .expect("MOIRAI_RS007_EVAL_EXAMPLES"),
            shrink: env_or("MOIRAI_RS007_EVAL_SHRINK", "0")
                .parse()
                .expect("MOIRAI_RS007_EVAL_SHRINK"),
            cost_n: list("MOIRAI_RS007_EVAL_COST_N", "150,300")
                .iter()
                .map(|x| x.parse().expect("MOIRAI_RS007_EVAL_COST_N"))
                .collect(),
        }
    }

    /// The variant first, then the references.
    pub(crate) fn rules(&self) -> Vec<Rule> {
        let mut v = vec![self.variant];
        v.extend(self.refs.iter().copied());
        v
    }
}

/// A table: a label column, then one column per header.
fn table(out: &mut String, head: &[String], rows: &[(String, Vec<String>)]) {
    let w = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(10)
        .max(10);
    let cw: Vec<usize> = (0..head.len())
        .map(|j| {
            rows.iter()
                .map(|(_, c)| c.get(j).map_or(0, |x| x.chars().count()))
                .max()
                .unwrap_or(0)
                .max(head[j].chars().count())
                .max(8)
        })
        .collect();
    let _ = write!(out, "{:w$}", "");
    for (j, h) in head.iter().enumerate() {
        let _ = write!(out, "  {:>width$}", h, width = cw[j]);
    }
    out.push('\n');
    for (l, cells) in rows {
        let _ = write!(out, "{l:w$}");
        for (j, c) in cells.iter().enumerate() {
            let _ = write!(out, "  {:>width$}", c, width = cw[j]);
        }
        out.push('\n');
    }
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 {
        "-".into()
    } else {
        format!("{:.1}", 100.0 * a as f64 / b as f64)
    }
}

/// One absolute row: its label, its machine-readable key and its value per variant.
type AbsRow = (&'static str, &'static str, Box<dyn Fn(&Abs) -> String>);

/// One cost row: its label, its machine-readable key and its value.
type CostLine = (&'static str, &'static str, fn(&CostRow) -> String);

/// The absolute rows, one column per variant, with their machine-readable lines (`RS007EVAL <section> <metric>
/// <rule> <value>`).
fn abs_rows(
    section: &str,
    rules: &[Rule],
    abs: &[&Abs],
    tsv: &mut String,
) -> Vec<(String, Vec<String>)> {
    let k = |f: fn(&KStats) -> usize| move |a: &Abs| f(&a.total).to_string();
    let rows: Vec<AbsRow> = vec![
        (
            "histories",
            "histories",
            Box::new(|a: &Abs| a.histories.to_string()),
        ),
        (
            "model panics",
            "panics",
            Box::new(|a: &Abs| a.panics.to_string()),
        ),
        (
            "merge-family commands judged",
            "merges",
            Box::new(k(|s| s.n)),
        ),
        ("  landed", "landed", Box::new(k(|s| s.landed))),
        ("  staged", "staged", Box::new(k(|s| s.staged))),
        ("  refused", "refused", Box::new(k(|s| s.refused))),
        ("  up to date", "up_to_date", Box::new(k(|s| s.up_to_date))),
        (
            "staged HierarchyCycle, GENUINE (no forest exists)",
            "genuine",
            Box::new(k(|s| s.genuine)),
        ),
        (
            "staged HierarchyCycle, AVOIDABLE (a forest exists)",
            "avoidable",
            Box::new(k(|s| s.avoidable)),
        ),
        (
            "  of which a staged key is determined (I25' class)",
            "avoidable_determined",
            Box::new(k(|s| s.avoidable_det)),
        ),
        (
            "  avoidable share of HierarchyCycle stagings (%)",
            "avoidable_pct",
            Box::new(|a: &Abs| pct(a.total.avoidable, a.total.avoidable + a.total.genuine)),
        ),
        (
            "staged with no HierarchyCycle",
            "staged_other",
            Box::new(k(|s| s.staged_other)),
        ),
        (
            "staged candidate moves an unstaged determined key",
            "staged_candidate_wrong",
            Box::new(k(|s| s.cand_wrong)),
        ),
        (
            "staging no ours/theirs choice on its keys resolves",
            "staged_stuck",
            Box::new(k(|s| s.stuck)),
        ),
        (
            "landed WRONG (determined key elsewhere / two-sided at neither)",
            "wrong",
            Box::new(k(|s| s.wrong)),
        ),
        (
            "  on a one-sided key",
            "wrong_one_sided",
            Box::new(k(|s| s.wrong_one)),
        ),
        (
            "  on an untouched key (b = o = t)",
            "wrong_untouched",
            Box::new(k(|s| s.wrong_untouched)),
        ),
        (
            "  on a both-same key",
            "wrong_both_same",
            Box::new(k(|s| s.wrong_same)),
        ),
        (
            "  a two-sided key at neither side's value",
            "wrong_neither",
            Box::new(k(|s| s.wrong_neither)),
        ),
        (
            "  order only (every wrong key has the right parent)",
            "wrong_order_only",
            Box::new(k(|s| s.wrong_order_only)),
        ),
        (
            "ORACLE FAULTS (avoidable + wrong)",
            "faults",
            Box::new(|a: &Abs| (a.total.avoidable + a.total.wrong).to_string()),
        ),
        (
            "SECONDARY reading (touch = state or changeset, CRIT-2): genuine",
            "ch_genuine",
            Box::new(k(|s| s.genuine_ch)),
        ),
        (
            "  avoidable",
            "ch_avoidable",
            Box::new(k(|s| s.avoidable_ch)),
        ),
        ("  wrong", "ch_wrong", Box::new(k(|s| s.wrong_ch))),
        (
            "  faults (avoidable + wrong)",
            "ch_faults",
            Box::new(|a: &Abs| (a.total.avoidable_ch + a.total.wrong_ch).to_string()),
        ),
        (
            "histories with an oracle fault",
            "histories_with_fault",
            Box::new(|a: &Abs| a.with_fault.to_string()),
        ),
        (
            "two-sided keys landed at o's value",
            "two_sided_took_o",
            Box::new(k(|s| s.took_o)),
        ),
        (
            "two-sided keys landed at t's value",
            "two_sided_took_t",
            Box::new(k(|s| s.took_t)),
        ),
        (
            "stagings whose history's resolution aborted",
            "resolution_aborted",
            Box::new(k(|s| s.settle_aborted)),
        ),
        (
            "one-sided merges (base = tip(dst))",
            "one_sided",
            Box::new(k(|s| s.one_sided)),
        ),
        (
            "  staging a HierarchyCycle or landing off t's hierarchy",
            "one_sided_not_t",
            Box::new(k(|s| s.one_sided_not_t)),
        ),
        (
            "picks/reverts of a commit with no hierarchy entry",
            "pick_no_hier",
            Box::new(k(|s| s.pick_no_hier)),
        ),
        (
            "  staging a HierarchyCycle or landing off o's hierarchy",
            "pick_no_hier_not_o",
            Box::new(k(|s| s.pick_no_hier_not_o)),
        ),
        (
            "backstop resets (in commands)",
            "backstop",
            Box::new(k(|s| s.backstop)),
        ),
        (
            "backstop resets (in resolved-key derivations)",
            "backstop_derived",
            Box::new(|a: &Abs| a.derived_backstop.to_string()),
        ),
        ("candidate repairs", "repairs", Box::new(k(|s| s.repairs))),
        (
            "merges over a virtual base (b is the variant's own)",
            "virtual_base",
            Box::new(k(|s| s.virtual_n)),
        ),
        (
            "  of them avoidable / wrong",
            "virtual_base_faults",
            Box::new(|a: &Abs| format!("{}/{}", a.total.avoidable_virtual, a.total.wrong_virtual)),
        ),
        (
            "merges with a task not live in b, o and t (not judged)",
            "partial",
            Box::new(k(|s| s.partial_n)),
        ),
        (
            "  of them avoidable / wrong",
            "partial_faults",
            Box::new(|a: &Abs| format!("{}/{}", a.total.avoidable_partial, a.total.wrong_partial)),
        ),
        (
            "wall seconds (sum over histories)",
            "secs",
            Box::new(|a: &Abs| format!("{:.1}", a.secs)),
        ),
    ];
    let mut out = Vec::new();
    for (label, key, f) in &rows {
        let cells: Vec<String> = abs.iter().map(|a| f(a)).collect();
        for (r, c) in rules.iter().zip(&cells) {
            let _ = writeln!(tsv, "RS007EVAL\t{section}\t{key}\t{}\t{c}", r.name());
        }
        out.push((label.to_string(), cells));
    }
    for kind in Kind::MERGES {
        let cells: Vec<String> = abs
            .iter()
            .map(|a| {
                a.by_kind.get(&kind).map_or("-".into(), |s| {
                    format!("{}/{}/{}/{}", s.n, s.genuine, s.avoidable, s.wrong)
                })
            })
            .collect();
        if cells.iter().all(|c| c == "-") {
            continue;
        }
        for (r, c) in rules.iter().zip(&cells) {
            let _ = writeln!(
                tsv,
                "RS007EVAL\t{section}\tkind_{}\t{}\t{c}",
                kind.name(),
                r.name()
            );
        }
        out.push((
            format!("{} (n/genuine/avoidable/wrong)", kind.name()),
            cells,
        ));
    }
    out
}

/// The relative rows of the variant against one reference.
fn rel_rows(
    section: &str,
    v: Rule,
    r: Rule,
    rel: &Rel,
    tsv: &mut String,
) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for d in Div::ALL {
        let (tot, p) = rel.of(d);
        let key = format!("{d:?}").to_lowercase();
        let _ = writeln!(
            tsv,
            "RS007EVAL\t{section}\trel_{key}\t{}_vs_{}\t{tot}\t{}\t{}\t{}\t{}",
            v.name(),
            r.name(),
            p[0],
            p[1],
            p[2],
            p[3]
        );
        out.push((
            d.name().to_string(),
            vec![
                tot.to_string(),
                p[0].to_string(),
                p[1].to_string(),
                p[2].to_string(),
                p[3].to_string(),
            ],
        ));
    }
    let _ = writeln!(
        tsv,
        "RS007EVAL\t{section}\trel_history_faults\t{}_vs_{}\t{}\t{}\t{}",
        v.name(),
        r.name(),
        rel.fewer,
        rel.equal,
        rel.more
    );
    out.push((
        "whole history: variant has fewer / as many / more oracle faults".into(),
        vec![
            format!("{}/{}/{}", rel.fewer, rel.equal, rel.more),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ],
    ));
    out
}

/// Greedy shrink: drops one event at a time while `pred` holds.
fn shrink(h: &Hist, pred: &dyn Fn(&Hist) -> bool) -> Hist {
    let mut cur = h.clone();
    loop {
        let mut changed = false;
        let mut i = cur.ops.len();
        while i > 0 {
            i -= 1;
            let mut t = cur.clone();
            t.ops.remove(i);
            if !t.ops.is_empty() && pred(&t) {
                cur = t;
                changed = true;
                i = i.min(cur.ops.len());
            }
        }
        if !changed {
            return cur;
        }
    }
}

/// One example of a category: where it came from and its text.
struct Example {
    hist: Hist,
    head: String,
    text: String,
}

fn print_examples(
    out: &mut String,
    title: &str,
    ex: &[Example],
    shrink_n: usize,
    pred: &dyn Fn(&Hist) -> bool,
) {
    if ex.is_empty() {
        return;
    }
    let _ = writeln!(out, "  -- examples: {title}");
    for (i, e) in ex.iter().enumerate() {
        let _ = writeln!(out, "    {}: {}", e.head, e.text);
        let _ = writeln!(out, "      ops ({}): {:?}", e.hist.ops.len(), e.hist.ops);
        if i < shrink_n {
            let m = shrink(&e.hist, pred);
            let _ = writeln!(out, "      shrunk ({}): {:?}", m.ops.len(), m.ops);
        }
    }
}

/// The corpus part of the report.
fn report_corpus(cfg: &Cfg, out: &mut String, tsv: &mut String) {
    let rules = cfg.rules();
    let cases = corpus();
    let _ = writeln!(out, "\n== RS-007 eval: corpus ({} cases) ==", cases.len());
    let logs: Vec<Vec<Log>> = cases
        .iter()
        .map(|c| rules.iter().map(|r| run_caught(&c.hist, *r)).collect())
        .collect();
    // Summary: one row per focus merge, one column per variant.
    let word = |v: &Option<Verdict>, e: &Entry| -> String {
        match v {
            Some(Verdict::LandedOk { .. }) => "ok".to_string(),
            Some(Verdict::LandedWrong { .. }) => "WRONG".into(),
            Some(Verdict::Avoidable { .. }) => "AVOIDABLE".into(),
            Some(Verdict::Genuine { .. }) => "genuine".into(),
            Some(Verdict::StagedOther) => "staged-other".into(),
            Some(Verdict::Refused) => "refused".into(),
            Some(Verdict::UpToDate) => "up-to-date".into(),
            None => got_text(&e.got),
        }
    };
    let short = |e: &Entry| -> String {
        let mut s = word(&e.verdict, e);
        if e.verdict_ch.is_some() && word(&e.verdict_ch, e) != s {
            s = format!("{s}/ch:{}", word(&e.verdict_ch, e));
        }
        if e.backstop > 0 { format!("{s}+bs") } else { s }
    };
    let mut rows = Vec::new();
    for (c, ls) in cases.iter().zip(&logs) {
        let fe: Vec<Vec<&Entry>> = ls.iter().map(|l| focus_entries(c, l)).collect();
        let n = fe.iter().map(Vec::len).max().unwrap_or(0);
        for j in 0..n {
            let label = fe
                .iter()
                .find_map(|f| f.get(j))
                .map(|e| format!("{} [{}] {}", c.id, e.op, e.kind.name()))
                .unwrap_or_default();
            let cells: Vec<String> = fe
                .iter()
                .map(|f| f.get(j).map_or("-".to_string(), |e| short(e)))
                .collect();
            for (r, x) in rules.iter().zip(&cells) {
                let _ = writeln!(tsv, "RS007EVAL\tcorpus\t{label}\t{}\t{x}", r.name());
            }
            rows.push((label, cells));
        }
    }
    let head: Vec<String> = rules.iter().map(|r| r.name().to_string()).collect();
    table(out, &head, &rows);
    let _ = writeln!(
        out,
        "(+bs: the backstop fired; /ch:x: the secondary (changeset-touch) reading says x; columns: the variant under test first)"
    );
    for (c, ls) in cases.iter().zip(&logs) {
        let _ = writeln!(out, "\n-- {} [{}]: {}", c.id, c.source, c.note);
        let _ = writeln!(out, "   ops: {:?}", c.hist.ops);
        let fe: Vec<Vec<&Entry>> = ls.iter().map(|l| focus_entries(c, l)).collect();
        let n = fe.iter().map(Vec::len).max().unwrap_or(0);
        for j in 0..n {
            let tris: Vec<Option<&Tri>> = fe
                .iter()
                .map(|f| f.get(j).and_then(|e| e.tri.as_deref()))
                .collect();
            let first = tris.iter().flatten().next();
            if let Some(t) = first {
                let e0 = fe.iter().find_map(|f| f.get(j)).expect("an entry");
                let _ = writeln!(
                    out,
                    "   [{}] {}: b {} | o {} | t {}",
                    e0.op,
                    e0.kind.name(),
                    hier_text(&t.b),
                    hier_text(&t.o),
                    hier_text(&t.t)
                );
                let _ = writeln!(out, "       oracle: {}", expectation_text(t));
                let differ = tris
                    .iter()
                    .flatten()
                    .any(|x| (&x.b, &x.o, &x.t) != (&t.b, &t.o, &t.t));
                if differ {
                    let _ = writeln!(
                        out,
                        "       (the variants reach different b, o, t here: each is judged on its own)"
                    );
                }
            }
            for (r, f) in rules.iter().zip(&fe) {
                let Some(e) = f.get(j) else {
                    let _ = writeln!(
                        out,
                        "       {:8} (no such command: an earlier resolution aborted)",
                        r.name()
                    );
                    continue;
                };
                let res = e
                    .tri
                    .as_ref()
                    .map(|t| format!(" -> {}", hier_text(&t.r)))
                    .unwrap_or_default();
                let _ = writeln!(out, "       {:8} {}{res}", r.name(), entry_text(e));
                if !c.readings.is_empty()
                    && e.got == Got::Landed
                    && let Some(t) = &e.tri
                {
                    for (text, n, p) in &c.readings {
                        let got = t.r[*n as usize - 1].as_ref().map(|x| x.0);
                        if got == Some(*p) {
                            let _ = writeln!(
                                out,
                                "                CONTESTED: takes the reading \"{text}\""
                            );
                        }
                    }
                }
            }
        }
        if !c.readings.is_empty() {
            for (text, n, p) in &c.readings {
                let _ = writeln!(
                    out,
                    "   CONTESTED reading \"{text}\": #{n} {}",
                    p.map_or("at the root".to_string(), |p| format!("under #{p}"))
                );
            }
        }
    }
}

/// The search part of the report.
fn report_search(cfg: &Cfg, out: &mut String, tsv: &mut String) {
    let rules = cfg.rules();
    let mut all_abs: Vec<Abs> = vec![Abs::default(); rules.len()];
    let mut all_rel: Vec<Rel> = vec![Rel::default(); rules.len()];
    for g in &cfg.gens {
        let mut abs: Vec<Abs> = vec![Abs::default(); rules.len()];
        let mut rel: Vec<Rel> = vec![Rel::default(); rules.len()];
        let mut per_seed = String::new();
        let mut ex_avoid: Vec<Example> = Vec::new();
        let mut ex_wrong: Vec<Example> = Vec::new();
        let mut ex_rel: BTreeMap<(usize, &'static str), Vec<Example>> = BTreeMap::new();
        let t0 = std::time::Instant::now();
        for seed in &cfg.seeds {
            let hists = histories(*g, *seed, cfg.cases);
            let logs = run_all(&hists, &rules, cfg.threads);
            let mut seed_abs: Vec<Abs> = vec![Abs::default(); rules.len()];
            for (i, (h, ls)) in hists.iter().zip(&logs).enumerate() {
                for (j, l) in ls.iter().enumerate() {
                    abs[j].add(l);
                    all_abs[j].add(l);
                    seed_abs[j].add(l);
                }
                let v = &ls[0];
                for e in &v.entries {
                    let head = format!(
                        "{} seed {seed} history {i} event [{}] {}",
                        g.name(),
                        e.op,
                        e.kind.name()
                    );
                    let text = format!(
                        "{}; {}",
                        entry_text(e),
                        e.tri.as_ref().map(|t| tri_text(t)).unwrap_or_default()
                    );
                    match &e.verdict {
                        Some(Verdict::Avoidable { .. }) if ex_avoid.len() < cfg.examples => {
                            ex_avoid.push(Example {
                                hist: h.clone(),
                                head,
                                text,
                            })
                        }
                        Some(Verdict::LandedWrong { .. }) if ex_wrong.len() < cfg.examples => {
                            ex_wrong.push(Example {
                                hist: h.clone(),
                                head,
                                text,
                            })
                        }
                        _ => {}
                    }
                }
                for j in 1..rules.len() {
                    let c = compare(h, &ls[0], &ls[j]);
                    rel[j].add(&c);
                    all_rel[j].add(&c);
                    let cat = match (c.div, c.a_fault, c.r_fault) {
                        (Div::Same, ..) => None,
                        (_, true, false) => Some("judged worse (variant faults, ref clean)"),
                        (_, false, true) => Some("judged better (ref faults, variant clean)"),
                        (Div::Worse, ..) => Some("worse, not judged worse"),
                        (Div::Other, ..) => Some("other divergence"),
                        _ => None,
                    };
                    if let Some(cat) = cat {
                        let v = ex_rel.entry((j, cat)).or_default();
                        if v.len() < cfg.examples {
                            v.push(Example {
                                hist: h.clone(),
                                head: format!(
                                    "{} seed {seed} history {i} vs {}",
                                    g.name(),
                                    rules[j].name()
                                ),
                                text: c.msg.clone(),
                            });
                        }
                    }
                }
            }
            let _ = write!(per_seed, "  seed {seed}:");
            for (r, a) in rules.iter().zip(&seed_abs) {
                let _ = write!(
                    per_seed,
                    "  {} merges {} staged {} genuine {} avoidable {} wrong {}",
                    r.name(),
                    a.total.n,
                    a.total.staged,
                    a.total.genuine,
                    a.total.avoidable,
                    a.total.wrong
                );
            }
            per_seed.push('\n');
        }
        let section = g.name();
        let _ = writeln!(
            out,
            "\n== RS-007 eval: generator {section}, seeds {:?}, {} histories per seed ({:.0} s) ==",
            cfg.seeds,
            cfg.cases,
            t0.elapsed().as_secs_f64()
        );
        let head: Vec<String> = rules.iter().map(|r| r.name().to_string()).collect();
        let rows = abs_rows(section, &rules, &abs.iter().collect::<Vec<_>>(), tsv);
        table(out, &head, &rows);
        out.push_str(&per_seed);
        for j in 1..rules.len() {
            let _ = writeln!(
                out,
                "  -- {} against {} (first divergence per history; columns: total, variant fault & ref clean, variant clean & ref fault, both fault, neither)",
                rules[0].name(),
                rules[j].name()
            );
            let rows = rel_rows(section, rules[0], rules[j], &rel[j], tsv);
            let head: Vec<String> = ["total", "v-fault", "r-fault", "both", "neither"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            table(out, &head, &rows);
        }
        let v = rules[0];
        let pred_avoid = |h: &Hist| {
            run(h, v)
                .entries
                .iter()
                .any(|e| matches!(e.verdict, Some(Verdict::Avoidable { .. })))
        };
        let pred_wrong = |h: &Hist| {
            run(h, v)
                .entries
                .iter()
                .any(|e| matches!(e.verdict, Some(Verdict::LandedWrong { .. })))
        };
        print_examples(
            out,
            &format!("{} avoidable stagings", v.name()),
            &ex_avoid,
            cfg.shrink,
            &pred_avoid,
        );
        print_examples(
            out,
            &format!("{} wrong landings", v.name()),
            &ex_wrong,
            cfg.shrink,
            &pred_wrong,
        );
        for ((j, cat), ex) in &ex_rel {
            let r = rules[*j];
            let want = *cat;
            let pred = move |h: &Hist| {
                let c = compare(h, &run(h, v), &run(h, r));
                match want {
                    "judged worse (variant faults, ref clean)" => {
                        c.div != Div::Same && c.a_fault && !c.r_fault
                    }
                    "judged better (ref faults, variant clean)" => {
                        c.div != Div::Same && !c.a_fault && c.r_fault
                    }
                    "worse, not judged worse" => c.div == Div::Worse,
                    _ => c.div == Div::Other,
                }
            };
            print_examples(
                out,
                &format!("{} vs {}: {cat}", v.name(), r.name()),
                ex,
                cfg.shrink,
                &pred,
            );
        }
        // Each generator's section as soon as it is done.
        eprint!("{out}");
        out.clear();
    }
    if cfg.gens.len() > 1 {
        let _ = writeln!(
            out,
            "\n== RS-007 eval: all generators, seeds {:?}, {} histories per generator and seed ==",
            cfg.seeds, cfg.cases
        );
        let head: Vec<String> = rules.iter().map(|r| r.name().to_string()).collect();
        let rows = abs_rows("all", &rules, &all_abs.iter().collect::<Vec<_>>(), tsv);
        table(out, &head, &rows);
        for j in 1..rules.len() {
            let _ = writeln!(out, "  -- {} against {}", rules[0].name(), rules[j].name());
            let rows = rel_rows("all", rules[0], rules[j], &all_rel[j], tsv);
            let head: Vec<String> = ["total", "v-fault", "r-fault", "both", "neither"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            table(out, &head, &rows);
        }
    }
}

/// The cost part of the report.
fn report_cost(cfg: &Cfg, out: &mut String, tsv: &mut String) {
    let rules = cfg.rules();
    let _ = writeln!(out, "\n== RS-007 eval: cost (long lane; debug build) ==");
    let mut rows = Vec::new();
    for n in &cfg.cost_n {
        let rs: Vec<CostRow> = rules.iter().map(|r| cost(*r, *n)).collect();
        let lines: [CostLine; 9] = [
            ("total seconds", "total_s", |c| format!("{:.2}", c.total_s)),
            ("commits", "commits", |c| c.commits.to_string()),
            ("syncs of lane/x", "syncs", |c| c.syncs.to_string()),
            ("  staged (resolved to ours)", "staged", |c| {
                c.staged.to_string()
            }),
            ("  resolution aborted", "aborted", |c| c.aborted.to_string()),
            ("last 10 syncs, mean ms", "last10_ms", |c| {
                format!("{:.1}", c.last10_ms)
            }),
            ("slowest sync, ms", "max_ms", |c| format!("{:.1}", c.max_ms)),
            ("step keys from an empty memo, s", "cold_s", |c| {
                format!("{:.2}", c.cold_s)
            }),
            ("next sync after that, ms", "next_ms", |c| {
                format!("{:.1}", c.next_ms)
            }),
        ];
        for (label, key, f) in lines {
            let cells: Vec<String> = rs.iter().map(f).collect();
            for (r, c) in rules.iter().zip(&cells) {
                let _ = writeln!(tsv, "RS007EVAL\tcost_n{n}\t{key}\t{}\t{c}", r.name());
            }
            rows.push((format!("N={n}: {label}"), cells));
        }
    }
    let head: Vec<String> = rules.iter().map(|r| r.name().to_string()).collect();
    table(out, &head, &rows);
}

// ---------------------------------------------------------------------------------------------------------------------
// Tests.

/// The number of histories at the tier: `pr` at the PR tier, ×10 nightly, ×100 at the exit tier ([PLAN §2.1]).
fn tier(pr: usize) -> usize {
    match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => pr * 10,
        Ok("exit") => pr * 100,
        _ => pr,
    }
}

/// Every task live on a branch has a parent chain that ends at the root: the branch's hierarchy is a forest.
fn branch_is_forest(h: &Hier) -> bool {
    let p: Vec<Option<u32>> = h.iter().map(|v| v.as_ref().and_then(|x| x.0)).collect();
    let live: Vec<bool> = h.iter().map(Option::is_some).collect();
    forest(&p, &live)
}

/// The oracle alone, on hand-made states (no rule, no store).
#[test]
fn rs007_eval_oracle_classifies_by_state() {
    let v = |p: Option<u32>| Some((p, None::<String>));
    let root = v(None);
    // X1: o puts #1 under #2, t puts #2 under #1: both one-sided, no forest.
    let b = vec![root.clone(), root.clone(), root.clone()];
    let o = vec![v(Some(2)), root.clone(), root.clone()];
    let t = vec![root.clone(), v(Some(1)), root.clone()];
    let or = oracle(&b, &o, &t);
    assert_eq!(or.class[..2], [Class::OnlyO, Class::OnlyT]);
    assert_eq!(or.class[2], Class::Untouched);
    assert!(!or.forest);
    let tri = |r: Hier| Tri {
        b: b.clone(),
        o: o.clone(),
        t: t.clone(),
        r,
        virtual_base: false,
        one_sided: false,
        pick_no_hier: false,
        touch: None,
    };
    let hc = |n: u32| vec![(format!("#{n}.parent"), "HierarchyCycle".to_string())];
    assert_eq!(
        judge(
            &Got::Staged(vec![]),
            &tri(vec![v(Some(2)), root.clone(), root.clone()]),
            &hc(2)
        ),
        Verdict::Genuine {
            cand_wrong: false,
            stuck: false
        }
    );
    // A two-sided key makes a forest possible: o #1 under #2, t #1 under #3 and #2 under #1.
    let o2 = vec![v(Some(2)), root.clone(), root.clone()];
    let t2 = vec![v(Some(3)), v(Some(1)), root.clone()];
    let or = oracle(&b, &o2, &t2);
    assert_eq!(or.class[..2], [Class::BothDiff, Class::OnlyT]);
    assert!(or.forest, "#1 under #3 with #2 under #1 is a forest");
    let t3 = Tri {
        b: b.clone(),
        o: o2.clone(),
        t: t2.clone(),
        r: vec![v(Some(2)), root.clone(), root.clone()],
        virtual_base: false,
        one_sided: false,
        pick_no_hier: false,
        touch: None,
    };
    assert_eq!(
        judge(&Got::Staged(vec![]), &t3, &hc(2)),
        Verdict::Avoidable {
            on_determined: true,
            cand_wrong: false,
            stuck: false
        }
    );
    // A landing that takes o's #1 and drops t's #2: wrong on a one-sided key.
    assert_eq!(
        judge(&Got::Landed, &t3, &[]),
        Verdict::LandedWrong {
            one_sided: 1,
            untouched: 0,
            both_same: 0,
            neither: 0,
            order_only: false
        }
    );
    let ok = Tri {
        r: vec![v(Some(3)), v(Some(1)), root.clone()],
        ..t3.clone()
    };
    assert_eq!(
        judge(&Got::Landed, &ok, &[]),
        Verdict::LandedOk {
            took_o: 0,
            took_t: 1
        }
    );
    // A key equal in b, o and t that lands elsewhere is wrong (untouched); an order-only difference counts.
    let b4 = vec![v(Some(2)), root.clone(), root.clone()];
    let o4 = vec![
        Some((Some(2), Some("a1".to_string()))),
        root.clone(),
        root.clone(),
    ];
    let t4 = vec![v(Some(2)), root.clone(), v(Some(1))];
    let or = oracle(&b4, &o4, &t4);
    assert_eq!(or.class, vec![Class::OnlyO, Class::Untouched, Class::OnlyT]);
    let w = Tri {
        b: b4,
        o: o4,
        t: t4,
        r: vec![v(Some(2)), v(Some(3)), v(Some(1))],
        virtual_base: false,
        one_sided: false,
        pick_no_hier: false,
        touch: None,
    };
    assert_eq!(
        judge(&Got::Landed, &w, &[]),
        Verdict::LandedWrong {
            one_sided: 1,
            untouched: 1,
            both_same: 0,
            neither: 0,
            order_only: false
        }
    );
    // A dead task is not judged.
    let or = oracle(
        &vec![root.clone(), None],
        &vec![root.clone(), root.clone()],
        &vec![root.clone(), root.clone()],
    );
    assert_eq!(or.class, vec![Class::Untouched, Class::NotJudged]);
    assert!(or.partial && or.forest);
}

/// What a corpus case's focus merges must do under every acceptable rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Must {
    /// Stage a `HierarchyCycle`, and the oracle agrees no forest exists.
    Stage,
    /// Land, and the oracle finds nothing wrong.
    Land,
}

/// PR tier: the corpus cases whose outcome every acceptable rule must give, under the thread's default rule
/// (`MOIRAI_RS007_RULE`): the X1 genuine cycles and the order-only cycle of open point 35 (vi) and (iii) stage; the
/// swap in one transaction, the three-commit restructure, E1's one-sided merge into an unmoved `main` and the
/// cherry-pick of a commit with no hierarchy entry land, with nothing the oracle calls wrong.
#[test]
fn rs007_eval_corpus_invariants() {
    let rule = merge::default_rule();
    let musts = [
        ("X1", Must::Stage),
        ("X1-sync", Must::Stage),
        ("OP35-vi", Must::Stage),
        ("OP35-iii", Must::Stage),
        ("SWAP", Must::Land),
        ("RESTRUCT-sync", Must::Land),
        ("RESTRUCT-merge", Must::Land),
        ("E1", Must::Land),
        ("OP35-ii", Must::Land),
    ];
    for (id, must) in musts {
        let c = corpus_case(id).expect("a corpus case");
        let l = run(&c.hist, rule);
        assert!(l.panic.is_none(), "{id}: {:?}", l.panic);
        let fe = focus_entries(&c, &l);
        assert!(
            !fe.is_empty(),
            "{id}: no focus merge\n{}",
            trace_text(&c.hist, &l)
        );
        for e in fe {
            let ok = match must {
                Must::Stage => {
                    matches!(&e.got, Got::Staged(k) if k.iter().any(|k| k.ends_with("HierarchyCycle")))
                        && matches!(e.verdict, Some(Verdict::Genuine { .. }))
                }
                Must::Land => {
                    e.got == Got::Landed && matches!(e.verdict, Some(Verdict::LandedOk { .. }))
                }
            };
            assert!(
                ok,
                "{id} under {rule:?}: {must:?} expected\n{}",
                trace_text(&c.hist, &l)
            );
        }
    }
    // The order-only move of (vi) keeps theirs' order: the staged candidate holds #1 at theirs' value.
    let c = corpus_case("OP35-vi").expect("OP35-vi");
    let l = run(&c.hist, rule);
    let e = focus_entries(&c, &l)[0];
    let t = e.tri.as_ref().expect("judged");
    assert_eq!(
        t.r[0],
        t.t[0],
        "#1 takes theirs' order\n{}",
        trace_text(&c.hist, &l)
    );
    assert!(
        matches!(&e.got, Got::Staged(k) if !k.iter().any(|k| k.starts_with("#1."))),
        "the order-only move is never undone\n{}",
        trace_text(&c.hist, &l)
    );
}

/// PR tier: over random histories of every generator (16 per generator at the PR tier, seed fixed), under the thread's
/// default rule: the model never panics; every branch's hierarchy stays a forest; a one-sided merge (its base's commit
/// is tip(dst)) never stages a `HierarchyCycle` and lands src's hierarchy on every judged task (OQ-A-11 11.1 (A), I25′);
/// a revert or cherry-pick of a commit with no hierarchy entry never stages a `HierarchyCycle` and keeps dst's
/// hierarchy (RS-007's guarantee); and a history run twice gives the same log.
#[test]
fn rs007_eval_random_invariants() {
    let rule = merge::default_rule();
    for g in Gen::ALL {
        for (i, h) in histories(g, 1, tier(16)).iter().enumerate() {
            let l = run(h, rule);
            let where_ = || {
                format!(
                    "{} history {i} under {rule:?}\n{}",
                    g.name(),
                    trace_text(h, &l)
                )
            };
            assert!(l.panic.is_none(), "{}", where_());
            for snap in &l.snaps {
                for (b, hh) in snap {
                    assert!(branch_is_forest(hh), "{b} is not a forest: {}", where_());
                }
            }
            for e in &l.entries {
                let Some(t) = &e.tri else { continue };
                let hc = matches!(&e.got, Got::Staged(k) if k.iter().any(|k| k.ends_with("HierarchyCycle")));
                if t.one_sided {
                    assert!(
                        !hc && (e.got != Got::Landed || judged_eq(&t.r, &t.t, t)),
                        "a one-sided merge did not take src's hierarchy: {}",
                        where_()
                    );
                }
                if t.pick_no_hier {
                    assert!(
                        !hc && (e.got != Got::Landed || judged_eq(&t.r, &t.o, t)),
                        "a pick of a commit with no hierarchy entry moved dst's hierarchy: {}",
                        where_()
                    );
                }
            }
            if i < 4 {
                let again = run(h, rule);
                assert_eq!(
                    (&again.entries, &again.snaps),
                    (&l.entries, &l.snaps),
                    "not deterministic: {}",
                    where_()
                );
            }
        }
    }
}

/// The report: one table of every metric for the variant `MOIRAI_RS007_EVAL_RULE` (default `cand`) against the
/// references `MOIRAI_RS007_EVAL_VS`, over the corpus, `MOIRAI_RS007_EVAL_CASES` histories of each generator per seed
/// of `MOIRAI_RS007_EVAL_SEEDS`, and the long-lane cost ([`Cfg`] lists every variable). Run by hand:
/// `MOIRAI_RS007_EVAL_RULE=cand cargo test -p moirai-model --locked --lib rs007_eval_report -- --ignored --nocapture`.
/// Machine-readable lines start with `RS007EVAL`; `MOIRAI_RS007_EVAL_OUT=<path>` also writes the report there.
#[test]
#[ignore]
fn rs007_eval_report() {
    let cfg = Cfg::from_env();
    let mut out = String::new();
    let mut tsv = String::new();
    let _ = writeln!(
        out,
        "RS-007 evaluation: variant {} against {:?}; parts {:?}; cases {}; seeds {:?}; generators {:?}; threads {}",
        cfg.variant.name(),
        cfg.refs.iter().map(|r| r.name()).collect::<Vec<_>>(),
        cfg.parts,
        cfg.cases,
        cfg.seeds,
        cfg.gens.iter().map(|g| g.name()).collect::<Vec<_>>(),
        cfg.threads
    );
    let t0 = std::time::Instant::now();
    if cfg.parts.contains("corpus") {
        report_corpus(&cfg, &mut out, &mut tsv);
        eprint!("{out}");
        out.clear();
    }
    if cfg.parts.contains("search") {
        report_search(&cfg, &mut out, &mut tsv);
        eprint!("{out}");
        out.clear();
    }
    if cfg.parts.contains("cost") {
        report_cost(&cfg, &mut out, &mut tsv);
        eprint!("{out}");
        out.clear();
    }
    let _ = writeln!(out, "\n(total {:.0} s)", t0.elapsed().as_secs_f64());
    eprint!("{out}{tsv}");
    if let Ok(p) = std::env::var("MOIRAI_RS007_EVAL_OUT") {
        std::fs::write(&p, &tsv).expect("MOIRAI_RS007_EVAL_OUT is writable");
    }
}

/// A verbose trace of one history under each variant of `MOIRAI_RS007_EVAL_RULE` (a comma list; default
/// `current,wave3c,fromb`): `MOIRAI_RS007_EVAL_CASE` names a corpus case (`E6`), or a generated history as
/// `<generator>:<seed>:<index>` (`lanes:1:17`, the numbering of the report's examples). Run by hand, `--ignored
/// --nocapture`.
#[test]
#[ignore]
fn rs007_eval_trace() {
    let which = env_or("MOIRAI_RS007_EVAL_CASE", "E6");
    let rules: Vec<Rule> = env_or("MOIRAI_RS007_EVAL_RULE", "current,wave3c,fromb")
        .split(',')
        .map(|r| Rule::parse(r).expect("MOIRAI_RS007_EVAL_RULE"))
        .collect();
    let h = match corpus_case(&which) {
        Some(c) => {
            eprintln!("{} [{}]: {}", c.id, c.source, c.note);
            c.hist
        }
        None => {
            let p: Vec<&str> = which.split(':').collect();
            let (g, seed, i) = match p[..] {
                [g, s, i] => (
                    Gen::parse(g).expect("a generator"),
                    s.parse::<u8>().expect("a seed"),
                    i.parse::<usize>().expect("an index"),
                ),
                _ => panic!("MOIRAI_RS007_EVAL_CASE: a corpus id or <generator>:<seed>:<index>"),
            };
            histories(g, seed, i + 1).pop().expect("the history")
        }
    };
    let logs: Vec<Log> = rules.iter().map(|r| run_caught(&h, *r)).collect();
    for (r, l) in rules.iter().zip(&logs) {
        eprintln!("=== {} ({:.2} s)\n{}", r.name(), l.secs, trace_text(&h, l));
    }
    for j in 1..rules.len() {
        let c = compare(&h, &logs[0], &logs[j]);
        eprintln!(
            "=== {} against {}: {:?} at event {:?} (variant fault {}, ref fault {}; history faults {} / {}) {}",
            rules[0].name(),
            rules[j].name(),
            c.div,
            c.at,
            c.a_fault,
            c.r_fault,
            c.faults.0,
            c.faults.1,
            c.msg
        );
    }
}

/// Lists the generated histories in which a variant's merge of a kind gets a verdict: `MOIRAI_RS007_EVAL_RULE` (one
/// variant, default `current`), `MOIRAI_RS007_EVAL_GENS` (one generator, default `lanes`), `MOIRAI_RS007_EVAL_SEEDS`
/// (one seed, default 1), `MOIRAI_RS007_EVAL_CASES` (default 200), `MOIRAI_RS007_EVAL_KIND` (`sync`, `cross`,
/// `step0`, `into-main`, `based`, `criss`, `pick`, `revert` or `any`, default `any`), `MOIRAI_RS007_EVAL_VERDICT`
/// (`wrong`, `avoidable`, `genuine`, `fault`, `ok`; default `fault`), `MOIRAI_RS007_EVAL_EXAMPLES` (default 5). Each
/// hit prints `<generator>:<seed>:<index>` for [`rs007_eval_trace`]. Run by hand, `--ignored --nocapture`.
#[test]
#[ignore]
fn rs007_eval_find() {
    let rule =
        Rule::parse(&env_or("MOIRAI_RS007_EVAL_RULE", "current")).expect("MOIRAI_RS007_EVAL_RULE");
    let g = Gen::parse(&env_or("MOIRAI_RS007_EVAL_GENS", "lanes")).expect("MOIRAI_RS007_EVAL_GENS");
    let seed: u8 = env_or("MOIRAI_RS007_EVAL_SEEDS", "1")
        .parse()
        .expect("MOIRAI_RS007_EVAL_SEEDS");
    let cases: usize = env_or("MOIRAI_RS007_EVAL_CASES", "200")
        .parse()
        .expect("MOIRAI_RS007_EVAL_CASES");
    let kind = env_or("MOIRAI_RS007_EVAL_KIND", "any");
    let want = env_or("MOIRAI_RS007_EVAL_VERDICT", "fault");
    let max: usize = env_or("MOIRAI_RS007_EVAL_EXAMPLES", "5")
        .parse()
        .expect("MOIRAI_RS007_EVAL_EXAMPLES");
    let mut hits = 0;
    for (i, h) in histories(g, seed, cases).iter().enumerate() {
        let l = run_caught(h, rule);
        for e in &l.entries {
            if kind != "any" && e.kind.name() != kind {
                continue;
            }
            let ok = match (&e.verdict, want.as_str()) {
                (Some(Verdict::LandedWrong { .. }), "wrong") => true,
                (Some(Verdict::Avoidable { .. }), "avoidable") => true,
                (Some(Verdict::Genuine { .. }), "genuine") => true,
                (Some(Verdict::LandedOk { .. }), "ok") => true,
                (Some(v), "fault") => v.fault(),
                _ => false,
            };
            if ok {
                hits += 1;
                eprintln!(
                    "{}:{seed}:{i} event [{}] {:?}\n    {}\n    {}",
                    g.name(),
                    e.op,
                    h.ops[e.op],
                    entry_text(e),
                    e.tri.as_ref().map(|t| tri_text(t)).unwrap_or_default()
                );
                break;
            }
        }
        if hits >= max {
            break;
        }
    }
    eprintln!("{hits} histories listed");
}
