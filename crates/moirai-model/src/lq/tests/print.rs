//! The printer property of [LQ/canonical-ast §3.4]: `parse(print(a)) == a` for every S-AST `a`, in both display
//! spellings ([LQ/gql-spelling §4]), for reads, `TX` blocks and definitions.

use super::strat;
use crate::lq::parser::{ParseOptions, on_front_end_stack, parse_define, parse_read, parse_write};
use crate::lq::printer::{Spelling, print_define, print_read, print_tx};
use crate::lq::sexpr;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// Cases per property; the generated trees are deep (subqueries hold clauses), so the runner itself runs on the front
/// end's stack.
const CASES: u32 = 700;

fn run<S>(
    strategy: impl FnOnce() -> S + Send,
    check: impl Fn(S::Value) -> Result<(), TestCaseError> + Send,
) where
    S: Strategy,
{
    on_front_end_stack(move || {
        let mut runner = super::runner(CASES);
        if let Err(e) = runner.run(&strategy(), check) {
            panic!("{e}");
        }
    });
}

fn spelling(gql: bool) -> Spelling {
    if gql { Spelling::Gql } else { Spelling::Cypher }
}

#[test]
fn reads_round_trip() {
    run(
        || (strat::read(), any::<bool>()),
        |(r, gql)| {
            let text = print_read(&r, spelling(gql));
            match parse_read(&text, ParseOptions::default()) {
                Ok(p) => prop_assert!(
                    p.tree == r,
                    "text:\n{}\n got: {}\nwant: {}",
                    text,
                    sexpr::read(&p.tree),
                    sexpr::read(&r)
                ),
                Err(e) => prop_assert!(
                    false,
                    "text:\n{}\nerror: {}\nwant: {}",
                    text,
                    super::show(&text, &e),
                    sexpr::read(&r)
                ),
            }
            Ok(())
        },
    );
}

#[test]
fn transactions_round_trip() {
    run(
        || (strat::tx(), any::<bool>()),
        |(t, gql)| {
            let text = print_tx(&t, spelling(gql));
            match parse_write(&text, ParseOptions::default()) {
                Ok(p) => prop_assert!(
                    p.tree == t,
                    "text:\n{}\n got: {}\nwant: {}",
                    text,
                    sexpr::tx(&p.tree),
                    sexpr::tx(&t)
                ),
                Err(e) => prop_assert!(
                    false,
                    "text:\n{}\nerror: {}\nwant: {}",
                    text,
                    super::show(&text, &e),
                    sexpr::tx(&t)
                ),
            }
            Ok(())
        },
    );
}

#[test]
fn definitions_round_trip() {
    run(
        || (strat::define(), any::<bool>()),
        |(d, gql)| {
            let text = print_define(&d, spelling(gql));
            match parse_define(&text, ParseOptions::default()) {
                Ok(p) => prop_assert!(
                    p.tree == d,
                    "text:\n{}\n got: {}\nwant: {}",
                    text,
                    sexpr::define(&p.tree),
                    sexpr::define(&d)
                ),
                Err(e) => prop_assert!(
                    false,
                    "text:\n{}\nerror: {}\nwant: {}",
                    text,
                    super::show(&text, &e),
                    sexpr::define(&d)
                ),
            }
            Ok(())
        },
    );
}

/// [LQ/grammar-v1.ebnf §G.1]: every text the strict-GQL spelling mode accepts parses to the same S-AST, with the same
/// token stream, as in the default mode. The texts are printed in the GQL display spelling; those that keep a Cypher
/// spelling are refused by the mode and prove nothing, so the property also asserts that a fair share is accepted.
#[test]
fn strict_gql_texts_parse_as_in_the_default_mode() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let strict = ParseOptions { strict_gql: true };
    let accepted = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
    run(
        || (strat::read(), strat::tx(), strat::define()),
        |(r, t, d)| {
            let text = print_read(&r, Spelling::Gql);
            if let Ok(s) = parse_read(&text, strict) {
                accepted[0].fetch_add(1, Ordering::Relaxed);
                let p = parse_read(&text, ParseOptions::default());
                prop_assert!(
                    p.as_ref()
                        .is_ok_and(|p| p.tree == s.tree && p.tokens == s.tokens),
                    "text:\n{}\nstrict: {}",
                    text,
                    sexpr::read(&s.tree)
                );
            }
            let text = print_tx(&t, Spelling::Gql);
            if let Ok(s) = parse_write(&text, strict) {
                accepted[1].fetch_add(1, Ordering::Relaxed);
                let p = parse_write(&text, ParseOptions::default());
                prop_assert!(
                    p.as_ref()
                        .is_ok_and(|p| p.tree == s.tree && p.tokens == s.tokens),
                    "text:\n{}\nstrict: {}",
                    text,
                    sexpr::tx(&s.tree)
                );
            }
            let text = print_define(&d, Spelling::Gql);
            if let Ok(s) = parse_define(&text, strict) {
                accepted[2].fetch_add(1, Ordering::Relaxed);
                let p = parse_define(&text, ParseOptions::default());
                prop_assert!(
                    p.as_ref()
                        .is_ok_and(|p| p.tree == s.tree && p.tokens == s.tokens),
                    "text:\n{}\nstrict: {}",
                    text,
                    sexpr::define(&s.tree)
                );
            }
            Ok(())
        },
    );
    let accepted = accepted.map(|a| a.into_inner());
    println!("strict-GQL texts accepted of {CASES} (reads, TX blocks, definitions): {accepted:?}");
    assert!(
        accepted.iter().all(|&n| n >= CASES / 20),
        "strict-GQL texts accepted of {CASES} (reads, TX blocks, definitions): {accepted:?}"
    );
}

/// The layout of [LQ/gql-spelling §5.2] on a standard-library text.
#[test]
fn layout_of_a_definition() {
    let src = "DEFINE QUERY ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node BUDGET light AS {\n  MATCH (t:task)\n  WHERE t.ready\n    AND ($scope IS NULL OR t IN subtree($scope))\n    AND ($role IS NULL OR fits_role(t, $role))\n  RETURN t ORDER BY t.priority, t.topo, t.id LIMIT $limit\n}";
    let d = parse_define(src, ParseOptions::default()).unwrap().tree;
    assert_eq!(
        print_define(&d, Spelling::Cypher),
        "DEFINE QUERY ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node BUDGET light AS {\n  MATCH (t:task)\n  WHERE t.ready\n    AND ($scope IS NULL OR t IN subtree($scope))\n    AND ($role IS NULL OR fits_role(t, $role))\n  RETURN t ORDER BY t.priority, t.topo, t.id LIMIT $limit\n}"
    );
}

/// The display spellings of quantifiers ([LQ/gql-spelling §4.2]).
#[test]
fn quantifier_spellings() {
    let r = parse_read(
        "MATCH (a)-[:BLOCKS]->{2,}(b)-[:T*1..3]->(c)-[]->*(d)-->{2}(e) RETURN e",
        ParseOptions::default(),
    )
    .unwrap()
    .tree;
    assert_eq!(
        print_read(&r, Spelling::Cypher),
        "MATCH (a)-[:BLOCKS*2..]->(b)-[:T*1..3]->(c)-[*0..]->(d)-[*2]->(e)\nRETURN e"
    );
    assert_eq!(
        print_read(&r, Spelling::Gql),
        "MATCH (a)-[:BLOCKS]->{2,}(b)-[:T]->{1,3}(c)-[]->*(d)-[]->{2}(e)\nRETURN e"
    );
}

#[test]
fn tx_layout() {
    let t = parse_write("tx on lane/x key 'k' { match (t {id: #1}) expect 1 set t.a = 1; delete #2 reason 'x' } dry", ParseOptions::default())
        .unwrap()
        .tree;
    assert_eq!(
        print_tx(&t, Spelling::Cypher),
        "TX ON lane/x KEY 'k' {\n  MATCH (t {id: #1}) EXPECT 1 SET t.a = 1;\n  DELETE #2 REASON 'x'\n} DRY"
    );
}
