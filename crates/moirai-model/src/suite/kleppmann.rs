//! The lockstep search that accepts OQ-A-12 ([AR §11] OQ-A-12; [RULES/merge-table] RS-007, open point 35 (v);
//! `docs/spec/reviews/wave-3c-arbiter.md` "Checks run" item 5): one random history of moves, priority edits, syncs,
//! cross-lane merges and merges into `main` runs on two stores at once, each under its own RS-007 variant
//! ([`crate::merge::Rule`]), and the first merge whose outcome or hierarchy differs between them ends the history.
//! Stagings are resolved to the side the history names on every violation's key and continued, and aborted when a
//! resolution is refused or the merge stages again. The acceptance: RS-007 never stages a merge that the replay from B
//! lands, and its backstop for a cycle the kept keys close never fires.

use super::*;
use crate::api::{Data, Outcome};
use crate::merge::{BACKSTOP, RULE, Rule};
use crate::tx::Take;
use std::cell::RefCell;

/// The branches of the search: `main` and two lanes forked from it at the commit that creates the tasks.
const BRANCHES: [&str; 3] = ["main", "lane/x", "lane/y"];

#[derive(Clone, Debug)]
enum Op {
    /// On branch b, node #n under #p, or to the root.
    Move(usize, u32, Option<u32>),
    /// On branch b, #n's priority.
    Priority(usize, u32, i64),
    /// `sync` of lane 1 or 2, staging resolved to the side.
    Sync(usize, bool),
    /// `merge` of the lane into the other lane, staging resolved to the side.
    Cross(usize, bool),
    /// `merge` of the lane into `main`, staging resolved to the side.
    IntoMain(usize, bool),
}

fn op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    // A move of #n (1–4) under another of the four, or now and then to the root: moves that often close cycles.
    let under = |n: u32, d: Option<u32>| d.map(|d| (n - 1 + d) % 4 + 1);
    let mv = (
        0usize..3,
        1u32..=4,
        prop_oneof![4 => (1u32..=3).prop_map(Some), 1 => Just(None)],
    )
        .prop_map(move |(b, n, d)| Op::Move(b, n, under(n, d)));
    prop_oneof![
        8 => mv,
        1 => (0usize..3, 1u32..=4, 0i64..4).prop_map(|(b, n, k)| Op::Priority(b, n, k)),
        2 => (1usize..3, any::<bool>()).prop_map(|(l, o)| Op::Sync(l, o)),
        2 => (1usize..3, any::<bool>()).prop_map(|(l, o)| Op::Cross(l, o)),
        1 => (1usize..3, any::<bool>()).prop_map(|(l, o)| Op::IntoMain(l, o)),
    ]
}

/// What a merge-family command did, as the comparison reads it.
#[derive(Clone, Debug, PartialEq)]
enum Got {
    /// Landed (or up to date): the parents of #1 to #4 on the destination.
    Landed(Vec<Option<Nid>>),
    /// Staged: the staging ref and its violations' keys.
    Staged(String, Vec<String>),
    /// Refused, with the error code.
    Refused(String),
}

/// How a search ended its histories.
#[derive(Debug, Default)]
struct Stats {
    histories: usize,
    /// Merge-family commands compared.
    merges: usize,
    /// Of them, those both variants staged alike (then resolved and continued alike).
    staged: usize,
    /// The variant under test stages where the other lands: the acceptance failure.
    worse: usize,
    /// The variant under test lands where the other stages.
    better: usize,
    /// Both land, with different hierarchies.
    both_land: usize,
    /// Both stage, on different keys.
    both_stage: usize,
    /// The variant under test's backstop resets.
    backstop: usize,
}

/// Runs `f` on a store under `rule`.
fn under<T>(rule: Rule, f: impl FnOnce() -> T) -> T {
    let was = RULE.with(|r| r.replace(rule));
    let out = f();
    RULE.with(|r| r.set(was));
    out
}

fn parents(s: &S, branch: &str) -> Vec<Option<Nid>> {
    let st =
        s.st.dag
            .state_at(s.st.dag.live(branch).and_then(|r| r.tip), &s.st.alloc);
    (1..=4).map(|n| st.nodes[&Nid(n)].parent).collect()
}

fn fresh_on(b: &str) -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch_on(b)
    }
}

/// Reads a merge-family reply: landed on `dst`, staged, or refused.
fn got(s: &S, r: &Reply, dst: &str) -> Got {
    match (&r.outcome, &r.data) {
        (Outcome::Ok, _) => Got::Landed(parents(s, dst)),
        (Outcome::Staged, Data::Merge(d)) => Got::Staged(
            d.staging_ref.clone().unwrap_or_default(),
            d.violations.iter().map(|v| v.key.clone()).collect(),
        ),
        _ => Got::Refused(r.error.as_ref().map(|e| e.code.clone()).unwrap_or_default()),
    }
}

/// `merge --continue` of a staging ref `merge/<dst>/from/<src>`.
fn continue_of(g: &str) -> (Cmd, String) {
    let rest = g.strip_prefix("merge/").expect("a staging ref");
    let (dst, src) = rest.split_once("/from/").expect("merge/<dst>/from/<src>");
    (
        Cmd::MergeContinue {
            src: Some(src.into()),
            into: Some(dst.into()),
        },
        dst.to_string(),
    )
}

/// Resolves every key of a staging to `take` and continues; aborts when a resolution is refused or the merge stages
/// again. The result is what the continue did, or `None` when it was aborted.
fn settle(s: &mut S, g: &str, keys: &[String], take: Take) -> Option<Got> {
    let (cont, dst) = continue_of(g);
    let mut ok = true;
    for k in keys.iter().filter(|k| *k != "-") {
        let r = s.run(
            tx(vec![Stmt::Resolve {
                key: k.clone(),
                take: take.clone(),
            }]),
            fresh_on(g),
        );
        ok &= r.outcome == Outcome::Ok;
    }
    let out = if ok {
        let r = s.run(cont, fresh_on(&dst));
        got(s, &r, &dst)
    } else {
        Got::Refused("resolve".into())
    };
    if matches!(out, Got::Landed(_)) {
        return Some(out);
    }
    let rest = g.strip_prefix("merge/").expect("a staging ref");
    let (dst, src) = rest.split_once("/from/").expect("merge/<dst>/from/<src>");
    s.ok(
        Cmd::MergeAbort {
            src: Some(src.into()),
            into: Some(dst.into()),
        },
        fresh_on(dst),
    );
    None
}

/// One history on two stores, store `a` under `ra` and store `b` under `rb`; the first divergence ends it.
fn lockstep(ops: &[Op], ra: Rule, rb: Rule, stats: &RefCell<Stats>) -> Result<(), String> {
    let setup = || {
        let mut s = S::base();
        s.ok(
            tx((1..=4)
                .map(|i| task(&format!("t{i}"), &format!("task {i}")))
                .collect()),
            orch(),
        );
        for l in ["x", "y"] {
            s.ok(
                Cmd::BranchCreate {
                    name: l.into(),
                    from: Some("main".into()),
                    kind: None,
                },
                orch(),
            );
        }
        s
    };
    let (mut a, mut b) = (setup(), setup());
    stats.borrow_mut().histories += 1;
    for op in ops {
        let (cmd, dst, take) = match op {
            Op::Move(br, n, p) => {
                let cmd = tx(vec![Stmt::Move {
                    target: Target::Id(Nid(*n)),
                    under: p.map(|p| Target::Id(Nid(p))),
                    position: None,
                }]);
                let ctx = fresh_on(BRANCHES[*br]);
                under(ra, || a.run(cmd.clone(), ctx.clone()));
                under(rb, || b.run(cmd, ctx));
                continue;
            }
            Op::Priority(br, n, k) => {
                let cmd = tx(vec![set(*n, &[("priority", P::Int(*k))])]);
                let ctx = fresh_on(BRANCHES[*br]);
                under(ra, || a.run(cmd.clone(), ctx.clone()));
                under(rb, || b.run(cmd, ctx));
                continue;
            }
            Op::Sync(l, o) => (
                Cmd::Sync {
                    lane: Some(BRANCHES[*l].into()),
                    check: false,
                },
                BRANCHES[*l],
                *o,
            ),
            Op::Cross(l, o) => (
                Cmd::Merge {
                    src: BRANCHES[*l].into(),
                    into: Some(BRANCHES[3 - *l].into()),
                    policy: None,
                    strict: None,
                    base: None,
                    message: String::new(),
                },
                BRANCHES[3 - *l],
                *o,
            ),
            Op::IntoMain(l, o) => (
                Cmd::Merge {
                    src: BRANCHES[*l].into(),
                    into: Some("main".into()),
                    policy: None,
                    strict: None,
                    base: None,
                    message: String::new(),
                },
                "main",
                *o,
            ),
        };
        let take = if take { Take::Ours } else { Take::Theirs };
        let ctx = fresh_on(dst);
        let ga = under(ra, || {
            let r = a.run(cmd.clone(), ctx.clone());
            got(&a, &r, dst)
        });
        let gb = under(rb, || {
            let r = b.run(cmd, ctx);
            got(&b, &r, dst)
        });
        let mut st = stats.borrow_mut();
        st.merges += 1;
        match (&ga, &gb) {
            (x, y) if x == y => {
                if let Got::Staged(g, keys) = x {
                    st.staged += 1;
                    let sa = under(ra, || settle(&mut a, g, keys, take.clone()));
                    let sb = under(rb, || settle(&mut b, g, keys, take));
                    match (sa, sb) {
                        (None, None) => {}
                        (x, y) if x == y => {}
                        (Some(_), Some(_)) => {
                            st.both_land += 1;
                            return Ok(());
                        }
                        (None, Some(_)) => {
                            st.worse += 1;
                            return Err(format!(
                                "{op:?}: the variant re-stages a resolution that lands"
                            ));
                        }
                        (Some(_), None) => {
                            st.better += 1;
                            return Ok(());
                        }
                    }
                }
            }
            (Got::Landed(_), Got::Landed(_)) => {
                st.both_land += 1;
                return Ok(());
            }
            (Got::Staged(..), Got::Landed(_)) => {
                st.worse += 1;
                return Err(format!("{op:?}: {ga:?} where the other lands"));
            }
            (Got::Landed(_), Got::Staged(..)) => {
                st.better += 1;
                return Ok(());
            }
            _ => {
                st.both_stage += 1;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The number of histories: `MOIRAI_RS007_CASES` when set, else by `MOIRAI_TEST_TIER` ([PLAN §2.1]).
fn cases(pr: u32) -> u32 {
    if let Some(n) = std::env::var("MOIRAI_RS007_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return n;
    }
    match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => pr * 10,
        Ok("exit") => pr * 100,
        _ => pr,
    }
}

/// Runs the search of `ra` against `rb` and returns its counts; a history where `ra` stages and `rb` lands fails it.
fn search(ra: Rule, rb: Rule, pr: u32, seed: u8) -> Stats {
    use proptest::prelude::*;
    let mut s = *b"moirai-model/rs007/lockstep-seed";
    s[31] ^= seed;
    let mut runner = proptest::test_runner::TestRunner::new_with_rng(
        ProptestConfig {
            cases: cases(pr),
            failure_persistence: None,
            ..ProptestConfig::default()
        },
        proptest::test_runner::TestRng::from_seed(proptest::test_runner::RngAlgorithm::ChaCha, &s),
    );
    let stats = RefCell::new(Stats::default());
    BACKSTOP.with(|c| c.set(0));
    let result = runner.run(&proptest::collection::vec(op(), 1..24), |ops| {
        lockstep(&ops, ra, rb, &stats).map_err(proptest::test_runner::TestCaseError::fail)
    });
    let mut out = stats.into_inner();
    out.backstop = BACKSTOP.with(|c| c.get());
    eprintln!("RS-007 lockstep {ra:?} against {rb:?}: {out:?}");
    if let Err(e) = result {
        panic!("{e}");
    }
    out
}

/// The acceptance of OQ-A-12 ([AR §11] OQ-A-12; [RULES/merge-table] RS-007, open point 35 (v)): over random
/// histories, RS-007 never stages a merge that the replay from B lands (the rule spec sync 3 stated, and the fallback
/// of OQ-A-11), and its backstop for a cycle the kept keys close never fires.
#[test]
fn rs_007_never_stages_what_the_replay_from_b_lands() {
    let st = search(Rule::Current, Rule::FromB, 64, 1);
    assert_eq!(st.worse, 0, "{st:?}");
    assert_eq!(st.backstop, 0, "the backstop fired: {st:?}");
}

/// The search finds what it looks for: with `MOIRAI_RS007_CASES` large, wave 3c's rule against the replay from B
/// meets E6's class (`wave-3c-arbiter.md` W3C-ARB-1); run by hand, not in a tier.
#[test]
#[ignore]
fn wave_3c_against_the_replay_from_b() {
    let st = search(Rule::Wave3c, Rule::FromB, 64, 3);
    eprintln!("{st:?}");
}

/// RS-007 against wave 3c's rule (OQ-A-11 alone), the same way: it never stages a merge that wave 3c lands.
#[test]
fn rs_007_never_stages_what_wave_3c_lands() {
    let st = search(Rule::Current, Rule::Wave3c, 64, 2);
    assert_eq!(st.worse, 0, "{st:?}");
    assert_eq!(st.backstop, 0, "the backstop fired: {st:?}");
}
