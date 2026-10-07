//! Probes of the evaluation harness's candidate K2 `threeway` (`merge/cand.rs`): hand-made histories that pin what the
//! state-based three-way rule does where it differs from RS-007's replay, and the known weaknesses of the rule. Each
//! runs under `Rule::Cand` and, for comparison, `Rule::Current`, on the harness's own runner ([`run`]).

use super::rs007eval::{Ev, Got, Hist, Kind, Log, T, run, trace_text};
use crate::merge::Rule;

const M: &str = "main";
const X: &str = "lane/x";
const Y: &str = "lane/y";

/// The merge-family outcomes of a log, in order.
fn outcomes(l: &Log) -> Vec<(usize, Kind, Got)> {
    l.entries
        .iter()
        .filter(|e| e.kind != Kind::Edit)
        .map(|e| (e.op, e.kind, e.got.clone()))
        .collect()
}

/// The hierarchy (parent of #1..#n, `None` for a root) of branch `b` after the last event.
fn parents(l: &Log, b: &str) -> Vec<Option<u32>> {
    let snap = l.snaps.last().expect("a snapshot");
    let (_, h) = snap.iter().find(|(n, _)| n == b).expect("the branch");
    h.iter().map(|v| v.as_ref().and_then(|(p, _)| *p)).collect()
}

fn hist(tasks: u32, lanes: &[&'static str], ops: Vec<Ev>) -> Hist {
    Hist {
        tasks,
        lanes: lanes.to_vec(),
        ops,
    }
}

/// A two-sided key whose later move would close a cycle with keys only one side changed lands at the other side's
/// (earlier) value, with no staging: main puts #4 under #2, then #1 under #3; lane/x puts #2 under #1, then #1 under
/// #4; `sync lane/x` lands with #1 under #3 (main's), #2 under #1 and #4 under #2. RS-007's replay undoes lane/x's
/// later move of #1 and stages `#1.parent`.
#[test]
fn k2_a_later_two_sided_move_that_closes_a_cycle_is_skipped_and_lands() {
    use Ev::*;
    let h = hist(
        4,
        &[X],
        vec![
            Mv(M, 4, Some(2)),
            Mv(M, 1, Some(3)),
            Mv(X, 2, Some(1)),
            Mv(X, 1, Some(4)),
            Sync(X, T::Ours),
        ],
    );
    let c = run(&h, Rule::Cand);
    assert_eq!(
        outcomes(&c),
        vec![(4, Kind::Sync, Got::Landed)],
        "{}",
        trace_text(&h, &c)
    );
    assert_eq!(parents(&c, X), vec![Some(3), Some(1), None, Some(2)]);
    let r = run(&h, Rule::Current);
    assert!(
        matches!(&outcomes(&r)[0].2, Got::Staged(k) if k.iter().any(|s| s.starts_with("#1.parent"))),
        "{}",
        trace_text(&h, &r)
    );
}

/// CRIT-2's smallest case: lane/x puts #2 under #1, main puts #1 under #2, lane/x puts #2 back at the root; `sync
/// lane/x` lands main's move (lane/x holds #2 where b does). Every replay variant stages `#1.parent`.
#[test]
fn k2_a_move_away_and_back_is_no_touch() {
    use Ev::*;
    let h = hist(
        2,
        &[X],
        vec![
            Mv(X, 2, Some(1)),
            Mv(M, 1, Some(2)),
            Mv(X, 2, None),
            Sync(X, T::Ours),
        ],
    );
    let c = run(&h, Rule::Cand);
    assert_eq!(
        outcomes(&c),
        vec![(3, Kind::Sync, Got::Landed)],
        "{}",
        trace_text(&h, &c)
    );
    assert_eq!(parents(&c, X), vec![Some(2), None]);
}

/// Weakness (shared with RS-007): MR-040 compares whole (parent, order) values, so a later order-only move on one side
/// beats an earlier parent change on the other: main puts #1 and #2 under #3 before the fork; lane/x puts #1 under #4;
/// main then moves #1 first under #3 (an order-only move); `sync lane/x` lands #1 under #3 and lane/x's move of #1 is
/// lost, under both rules.
#[test]
fn k2_a_later_order_only_move_beats_an_earlier_parent_change() {
    use Ev::*;
    let h = hist(
        4,
        &[],
        vec![
            Mv(M, 1, Some(3)),
            Mv(M, 2, Some(3)),
            Fork(X, M),
            Mv(X, 1, Some(4)),
            Reorder(M, 1, super::rs007eval::Pos::First),
            Sync(X, T::Ours),
        ],
    );
    for rule in [Rule::Cand, Rule::Current] {
        let l = run(&h, rule);
        assert_eq!(
            outcomes(&l),
            vec![(5, Kind::Sync, Got::Landed)],
            "{}",
            trace_text(&h, &l)
        );
        assert_eq!(
            parents(&l, X)[0],
            Some(3),
            "{rule:?}: {}",
            trace_text(&h, &l)
        );
    }
}

/// Weakness: the staged key of a cycle no choice avoids is the one-sided key with the latest origin, whatever side it
/// is on, so a history's blanket `--take ours` can re-close the cycle. E6's final merge (`merge lane/y --into lane/x`)
/// stages `#4.parent` (main's move, reached through lane/x's sync), and `ours` for it is main's value, which closes
/// the cycle again: the resolution is refused. RS-007 as prototyped landed a wrong value there.
#[test]
fn k2_e6_stages_a_key_whose_ours_closes_the_cycle() {
    use Ev::*;
    let h = hist(
        4,
        &[X, Y],
        vec![
            Mv(Y, 2, Some(3)),
            Mv(X, 1, Some(3)),
            Mv(M, 4, Some(2)),
            Mv(X, 3, Some(4)),
            Mv(M, 2, Some(1)),
            Merge(X, Y, T::Ours),
            Sync(X, T::Ours),
            Merge(Y, X, T::Ours),
        ],
    );
    let c = run(&h, Rule::Cand);
    let last = c
        .entries
        .iter()
        .rfind(|e| e.kind == Kind::Cross)
        .expect("the merge");
    assert_eq!(
        last.got,
        Got::Staged(vec!["#4.parent HierarchyCycle".into()]),
        "{}",
        trace_text(&h, &c)
    );
    assert_eq!(last.settled, Some(false), "{}", trace_text(&h, &c));
}

/// Prints the traces of the probes under both rules (run by hand, `--ignored --nocapture`).
#[test]
#[ignore]
fn k2_probe_traces() {
    use Ev::*;
    let hs = [hist(
        4,
        &[X],
        vec![
            Mv(M, 4, Some(2)),
            Mv(M, 1, Some(3)),
            Mv(X, 2, Some(1)),
            Mv(X, 1, Some(4)),
            Sync(X, T::Ours),
        ],
    )];
    for h in &hs {
        for rule in [Rule::Cand, Rule::Current] {
            println!("{rule:?}\n{}", trace_text(h, &run(h, rule)));
        }
    }
}

/// Lists the generated histories whose first divergence between `Cand` and a reference is a given class, with the
/// compare line and the variant's trace (run by hand, `--ignored --nocapture`): `K2_GEN` (narrow, lanes, wide, based),
/// `K2_SEED`, `K2_CASES`, `K2_REF` (current, wave3c, fromb), `K2_DIV` (worse, better, bothland, order, bothstage,
/// resworse, resbetter, other), `K2_FAULTS` (`neither` to keep only divergences where neither side faults), `K2_N`
/// (how many to print).
#[test]
#[ignore]
fn k2_probe_divergences() {
    use super::rs007eval::{Div, Gen, compare, histories};
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let g = match env("K2_GEN", "narrow").as_str() {
        "lanes" => Gen::Lanes,
        "wide" => Gen::Wide,
        "based" => Gen::Based,
        _ => Gen::Narrow,
    };
    let seed: u8 = env("K2_SEED", "1").parse().unwrap();
    let n: usize = env("K2_CASES", "300").parse().unwrap();
    let r = Rule::parse(&env("K2_REF", "current")).unwrap();
    let want = match env("K2_DIV", "bothland").as_str() {
        "worse" => Div::Worse,
        "better" => Div::Better,
        "order" => Div::BothLandOrder,
        "bothstage" => Div::BothStage,
        "resworse" => Div::ResolveWorse,
        "resbetter" => Div::ResolveBetter,
        "other" => Div::Other,
        _ => Div::BothLand,
    };
    let neither = env("K2_FAULTS", "") == "neither";
    let mut left: usize = env("K2_N", "3").parse().unwrap();
    for (i, h) in histories(g, seed, n).iter().enumerate() {
        let (a, b) = (run(h, Rule::Cand), run(h, r));
        let c = compare(h, &a, &b);
        if c.div != want || (neither && (c.a_fault || c.r_fault)) {
            continue;
        }
        println!(
            "=== {g:?}:{seed}:{i} at {:?}: {}\n--- cand\n{}--- {r:?}\n{}",
            c.at,
            c.msg,
            trace_text(h, &a),
            trace_text(h, &b)
        );
        left -= 1;
        if left == 0 {
            break;
        }
    }
}
