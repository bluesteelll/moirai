//! The refused forms of [LQ/grammar-v1.ebnf §R], the parser decisions of Annex P that raise codes, the strict-GQL
//! refusals of Annex G, the nesting limit (P13) and the recovery rules (P14): each case asserts the first error's code
//! and position, as the `fixtures/lq` error cases do.

use super::{first_err, first_err_tx, sx};
use crate::lq::diag::Code::{self, *};
use crate::lq::parser::{ParseOptions, parse_define, parse_read, parse_write};

fn at(src: &str) -> (Code, u32, u32) {
    first_err(src, false)
}

#[test]
fn annex_r_refused_forms() {
    let cases: &[(&str, (Code, u32, u32))] = &[
        ("MERGE (n:task {title: 'x'}) RETURN n", (E004, 1, 1)),
        ("MATCH (n) DETACH DELETE n", (E004, 1, 11)),
        ("MATCH p = (a)-[:BLOCKS]->(b) RETURN p", (E113, 1, 7)),
        ("MATCH shortestPath((a)-->(b)) RETURN a", (E004, 1, 7)),
        ("MATCH (a) RETURN nodes(a)", (E004, 1, 18)),
        ("MATCH (a) RETURN relationships(a)", (E004, 1, 18)),
        ("MATCH SHORTEST 1 (a)-->(b) RETURN b", (E004, 1, 7)),
        ("MATCH ANY SHORTEST (a)-->(b) RETURN b", (E004, 1, 7)),
        ("MATCH ALL PATHS (a)-->(b) RETURN b", (E004, 1, 7)),
        ("MATCH REPEATABLE ELEMENTS (a) RETURN a", (E004, 1, 7)),
        ("MATCH (a) RETURN a SKIP 5", (E004, 1, 20)),
        (
            "MATCH (a) RETURN a ORDER BY a OFFSET 5 LIMIT 1",
            (E004, 1, 31),
        ),
        (
            "MATCH (a) CALL { MATCH (b) RETURN b } RETURN a",
            (E004, 1, 11),
        ),
        ("MATCH (a) NEXT MATCH (b) RETURN b", (E004, 1, 11)),
        (
            "MATCH (a) FOREACH (x IN [1] | SET a.x = x) RETURN a",
            (E004, 1, 11),
        ),
        ("MATCH (a) FOR x IN a.l RETURN x", (E004, 1, 11)),
        ("MATCH (a) LET x = 1 RETURN x", (E004, 1, 11)),
        ("MATCH (a) FILTER a.x > 1 RETURN a", (E004, 1, 11)),
        ("RETURN [x IN [1, 2] WHERE x > 1 | x]", (E004, 1, 8)),
        ("RETURN [1 | 2]", (E004, 1, 11)),
        ("MATCH (a) RETURN a.l[0]", (E004, 1, 21)),
        ("MATCH (a) RETURN single(x IN a.l WHERE x)", (E004, 1, 18)),
        ("MATCH (a) WHERE a.x XOR a.y RETURN a", (E004, 1, 21)),
        ("MATCH (a) RETURN a.x % 2", (E004, 1, 22)),
        ("MATCH (a:note:rule) RETURN a", (E004, 1, 14)),
        ("MATCH (a) WHERE a:note:rule RETURN a", (E004, 1, 23)),
        (
            "MATCH (r:rule) WHERE r.applies('x') RETURN r",
            (E004, 1, 24),
        ),
        ("MATCH (a) WHERE a.x = NULL RETURN a", (E118, 1, 17)),
        ("MATCH (a) WHERE NULL <> a.x RETURN a", (E118, 1, 17)),
        ("MATCH (a) WHERE a.x != null RETURN a", (E118, 1, 17)),
        ("MATCH (a {x: null}) RETURN a", (E118, 1, 11)),
        ("MATCH (a)-[e {x: NULL}]->(b) RETURN a", (E118, 1, 15)),
        ("MATCH (a) WHERE a.t =~ 'x.*' RETURN a", (E004, 1, 21)),
        ("MATCH (a) RETURN timestamp()", (E004, 1, 18)),
        ("MATCH (a) WHERE a IS LABELED task RETURN a", (E004, 1, 19)),
        ("MATCH (a) RETURN CAST(a.x AS INTEGER)", (E004, 1, 18)),
        ("CALL apoc.coll.sum([1])", (E004, 1, 6)),
        ("CALL DB.labels()", (E004, 1, 6)),
        ("LOAD CSV FROM 'x' AS row RETURN row", (E004, 1, 1)),
        ("SHOW INDEXES", (E004, 1, 1)),
        ("CREATE INDEX FOR (n:task) ON (n.title)", (E004, 1, 1)),
        ("DROP CONSTRAINT c", (E004, 1, 1)),
        ("MATCH (a)<-[:BLOCKS]->(b) RETURN a", (E004, 1, 10)),
        ("MATCH (a)<-->(b) RETURN a", (E004, 1, 10)),
        ("MATCH (a) SET a.x = 1", (E006, 1, 11)),
        ("CREATE (n:task {title: 'x'})", (E006, 1, 1)),
        ("MATCH (a) DELETE a", (E006, 1, 11)),
        ("MATCH (a) REMOVE a.x RETURN a", (E006, 1, 11)),
        ("INSERT (n:task)", (E006, 1, 1)),
        ("MOVE #1 UNDER #2", (E006, 1, 1)),
        ("REOPEN #1 REASON 'x'", (E006, 1, 1)),
        ("PATCH #1.body REMOVE 'a' ADD 'b'", (E006, 1, 1)),
        ("RESOLVE 'k' TAKE OURS", (E006, 1, 1)),
        ("DEFINE QUERY q() AS { MATCH (n) RETURN n }", (E006, 1, 1)),
        ("DROP QUERY q", (E006, 1, 1)),
        ("ASSERT 1 = 1", (E006, 1, 1)),
        ("TX { SET #1.a = 1 }", (E006, 1, 1)),
        ("CALL tx.complete(#89, outcome: 'done')", (E006, 1, 6)),
        (
            "MATCH (a) WHERE EXISTS { USE main MATCH (b) } RETURN a",
            (E308, 1, 26),
        ),
        (
            "MATCH (a) WHERE COUNT { MATCH (b) USE main RETURN b } > 0 RETURN a",
            (E308, 1, 35),
        ),
        ("MATCH (a) USE main RETURN a", (E001, 1, 11)),
        ("MATCH (a) RETURN a WHERE a.x = 1", (E001, 1, 20)),
        (
            "MATCH (a) WITH a ORDER BY a.x LIMIT 3 WHERE a.y RETURN a",
            (E001, 1, 39),
        ),
        ("MATCH (a) RETURN a;", (E005, 1, 19)),
        ("MATCH (a) RETURN a MATCH (b) RETURN b", (E005, 1, 20)),
        ("MATCH (a)-[:T*2]->{1,3}(b) RETURN b", (E114, 1, 19)),
        ("MATCH (a)-[:T]->{3,1}(b) RETURN b", (E114, 1, 17)),
        ("MATCH (a)-[:T*3..1]->(b) RETURN b", (E114, 1, 14)),
        ("MATCH (a)-[:T]->{4294967296}(b) RETURN b", (E114, 1, 18)),
        ("MATCH (a)-[:T]->{1..3}(b) RETURN b", (E114, 1, 17)),
        ("MATCH (a)-[:T]->{p: 1}(b) RETURN b", (E001, 1, 18)),
        ("MATCH (a) WHERE a.x = :name RETURN a", (E001, 1, 23)),
        ("USE main@ MATCH (a) RETURN a", (E001, 1, 9)),
    ];
    for (src, want) in cases {
        assert_eq!(at(src), *want, "{src}");
    }
}

#[test]
fn annex_r_reflog_revisions_parse_and_leave_e117_to_the_binder() {
    // E117 is the binder's ([LQ/errors §5.1]); the parser reads reflog suffixes inside a definition.
    assert!(
        parse_define(
            "DEFINE QUERY q() AS { USE main@3 MATCH (n) RETURN n }",
            ParseOptions::default()
        )
        .is_ok()
    );
}

#[test]
fn transaction_decisions() {
    let cases: &[(&str, (Code, u32, u32))] = &[
        ("MATCH (a) RETURN a", (E001, 1, 1)),
        ("TX { }", (E009, 1, 6)),
        ("TX { MATCH (t:task) SET t.a = 1 }", (E007, 1, 21)),
        (
            "TX { MATCH (t) EXPECT 1 SET t.a = 1 CREATE (n:task) }",
            (E001, 1, 37),
        ),
        ("TX { CALL complete(#1) }", (E109, 1, 11)),
        ("TX { CALL std.x() }", (E109, 1, 11)),
        ("TX ON main ON main { SET #1.a = 1 }", (E001, 1, 12)),
        ("TX KEY 'a' KEY 'b' { SET #1.a = 1 }", (E001, 1, 12)),
        ("TX { SET #1.a = 1 } DRY x", (E005, 1, 25)),
        ("TX { SET #1.a = 1 } TX { SET #1.a = 1 }", (E005, 1, 21)),
        ("TX { DELETE #1 RELEASE RELEASE }", (E001, 1, 24)),
        ("TX { MERGE (n:task) }", (E004, 1, 6)),
        ("TX { DETACH DELETE #1 }", (E004, 1, 6)),
        ("TX { MATCH (a) EXPECT 1 DETACH DELETE a }", (E004, 1, 25)),
        ("TX { CREATE INDEX x }", (E004, 1, 6)),
        ("TX ON 'main' { SET #1.a = 1 }", (E001, 1, 7)),
        ("TX IF TIP main~x { SET #1.a = 1 }", (E003, 1, 11)),
        ("TX { UNLESS }", (E001, 1, 6)),
        ("TX { CREATE (n:task) UNLESS EXISTS { } }", (E001, 1, 38)),
    ];
    for (src, want) in cases {
        assert_eq!(first_err_tx(src, false), *want, "{src}");
    }
}

#[test]
fn query_decisions() {
    let cases: &[(&str, (Code, u32, u32))] = &[
        // P7: a CALL followed by more clauses needs YIELD with named columns.
        ("CALL blockers(#1) RETURN 1", (E001, 1, 19)),
        ("CALL blockers(#1) YIELD * RETURN 1", (E001, 1, 27)),
        ("MATCH (a) CALL blockers(a) RETURN a", (E001, 1, 28)),
        // P6: a group needs a quantifier.
        ("MATCH (x)((a)-->(b))(y) RETURN y", (E001, 1, 21)),
        // P8: an empty subquery.
        ("MATCH (a) WHERE EXISTS { } RETURN a", (E001, 1, 26)),
        (
            "MATCH (a) WHERE EXISTS { p = (a)-->() } RETURN a",
            (E113, 1, 26),
        ),
        // P2: a quote after USE.
        ("USE 'main' MATCH (a) RETURN a", (E001, 1, 5)),
        ("USE Main MATCH (a) RETURN a", (E003, 1, 5)),
        ("USE main~2x MATCH (a) RETURN a", (E003, 1, 5)),
        ("USE main /* c */ ~2 MATCH (a) RETURN a", (E001, 1, 18)),
        ("USE $r~1 MATCH (a) RETURN a", (E001, 1, 7)),
        ("CALL diff(a....b)", (E001, 1, 12)),
        ("CALL across(refs: [])", (E001, 1, 19)),
        ("CALL across(refs: [a..b])", (E001, 1, 21)),
        // Lexical codes reach the caller unchanged.
        ("MATCH (a) WHERE a.t = 'x RETURN a", (E002, 1, 23)),
        ("MATCH (a) WHERE a.id = #0 RETURN a", (E003, 1, 24)),
        ("MATCH (a) WHERE a.x = 1 ^ 2 RETURN a", (E001, 1, 25)),
        ("MATCH (a) WHERE ! a.x RETURN a", (E001, 1, 17)),
        ("MATCH (a) RETURN `", (E002, 1, 18)),
        ("MATCH (a) RETURN $", (E001, 1, 18)),
        ("MATCH (a) RETURN a\n  ORDER BY é", (E001, 2, 12)),
        // Reserved words are not variables.
        ("MATCH (a) UNWIND a.l AS match RETURN 1", (E001, 1, 25)),
        ("MATCH (a) RETURN a AS order", (E001, 1, 23)),
        ("MATCH (a) RETURN", (E001, 1, 17)),
        ("MATCH (a)", (E001, 1, 10)),
        ("RETURN 1 = 2 = 3", (E001, 1, 14)),
    ];
    for (src, want) in cases {
        assert_eq!(at(src), *want, "{src}");
    }
}

#[test]
fn strict_gql_mode_refuses_cypher_spellings() {
    let refused = [
        ("MATCH (a)-[:T*1..3]->(b) RETURN b", (E004, 1, 14)),
        ("MATCH (a) WHERE a.x != 1 RETURN a", (E004, 1, 21)),
        ("MATCH (a) WHERE (a)-->() RETURN a", (E004, 1, 17)),
        ("MATCH (a) WHERE exists((a)-->()) RETURN a", (E004, 1, 17)),
        ("MATCH (a) RETURN size((a)-->())", (E004, 1, 18)),
        ("MATCH (a) WHERE exists(a.p) RETURN a", (E004, 1, 17)),
        ("MATCH (a) RETURN toLower(a.t)", (E004, 1, 18)),
        ("MATCH (a) RETURN TOUPPER(a.t)", (E004, 1, 18)),
        ("MATCH (a) RETURN collect(a)", (E004, 1, 18)),
        ("MATCH (a) RETURN size(a.l)", (E004, 1, 18)),
    ];
    for (src, want) in refused {
        assert_eq!(first_err(src, true), want, "{src}");
        assert!(
            parse_read(src, ParseOptions::default()).is_ok(),
            "{src} is accepted by default"
        );
    }
    let strict = ParseOptions { strict_gql: true };
    for ok in [
        "MATCH (a)-[:T]->{1,3}(b) RETURN b",
        "MATCH (a) WHERE a.x <> 1 AND EXISTS { (a)-->() } AND COUNT { (a)-->() } > 1 RETURN a",
        "MATCH WALK (a)-->(b) WITH b UNWIND b.l AS x OPTIONAL MATCH (b)--(c) RETURN ALL lower(x), collect_list(x), cardinality(b.l), CASE WHEN x THEN 1 END",
    ] {
        assert!(parse_read(ok, strict).is_ok(), "{ok}");
    }
    assert_eq!(first_err_tx("TX { CREATE (n:task) }", true), (E004, 1, 6));
    assert_eq!(
        first_err_tx("TX { CREATE (#1)-[:BLOCKS]->(#2) }", true),
        (E004, 1, 6)
    );
    assert!(
        parse_write(
            "TX { INSERT (n:task); INSERT (#1)-[:BLOCKS]->(#2) }",
            strict
        )
        .is_ok()
    );
    // A text accepted in both modes parses to the same S-AST ([LQ/grammar-v1.ebnf §G.1]).
    let src = "MATCH (a)-[:T]->+(b) WHERE a.x <> 1 RETURN lower(b.t)";
    assert_eq!(
        sx(src),
        crate::lq::sexpr::read(&parse_read(src, strict).unwrap().tree)
    );
}

fn nested(n: usize, open: &str, close: &str, core: &str) -> String {
    format!("RETURN {}{core}{}", open.repeat(n), close.repeat(n))
}

#[test]
fn nesting_depth_limit_is_64() {
    for (open, close, entry) in [
        ("NOT ", "", 0),
        ("(", ")", 0),
        ("[", "]", 0),
        ("f(", ")", 1),
        ("{a: ", "}", 0),
    ] {
        let ok = nested(64, open, close, "1");
        assert!(
            parse_read(&ok, ParseOptions::default()).is_ok(),
            "{open} x 64"
        );
        let bad = nested(65, open, close, "1");
        let e = parse_read(&bad, ParseOptions::default()).unwrap_err();
        assert_eq!(e[0].code, E001, "{open} x 65");
        assert_eq!(e[0].message, "nesting deeper than 64 levels");
        // The 65th entry is the one reported.
        let col = 8 + 64 * open.len() as u32 + entry;
        assert_eq!(
            crate::lq::diag::line_col(&bad, e[0].span.unwrap().start),
            (1, col),
            "{open}"
        );
    }
    // Subquery braces and CASE count too.
    let ok = format!(
        "MATCH (a) WHERE {}a.x{} RETURN a",
        "EXISTS { MATCH (b) WHERE ".repeat(64),
        " }".repeat(64)
    );
    assert!(parse_read(&ok, ParseOptions::default()).is_ok());
    let bad = format!(
        "MATCH (a) WHERE {}a.x{} RETURN a",
        "EXISTS { MATCH (b) WHERE ".repeat(65),
        " }".repeat(65)
    );
    assert_eq!(
        parse_read(&bad, ParseOptions::default()).unwrap_err()[0].code,
        E001
    );
    // Binary operator chains are loops, not nesting.
    let long = format!("RETURN {}", vec!["1"; 20_000].join(" + "));
    let p = parse_read(&long, ParseOptions::default()).expect("a long chain parses");
    assert!(crate::lq::sexpr::read(&p.tree).len() > 20_000);
}

#[test]
fn recovery_reports_at_most_three_errors() {
    let e = parse_read(
        "MATCH (a WITH b.x AS y MATCH (c)) MATCH (d RETURN 1 UNWIND",
        ParseOptions::default(),
    )
    .unwrap_err();
    assert!(!e.is_empty() && e.len() <= 3, "{e:?}");
    assert_eq!(e[0].code, E001);
    let e = parse_write(
        "TX { SET a.b; SET c = 1; SET ; SET #1.x = 2 }",
        ParseOptions::default(),
    )
    .unwrap_err();
    assert!(e.len() == 3, "{e:?}");
    // A lexical error ends the pass.
    let e = parse_read(
        "MATCH (a) WHERE a.x = 'unterminated RETURN a; MATCH",
        ParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!((e.len(), e[0].code), (1, E002));
}

/// P5: when both alternatives of `(` fail, the error of the one that got further — a failing first node pattern is the
/// path alternative's failure — on a tie the path's, except that P13's nesting limit is reported as P13's E001.
#[test]
fn p5_the_alternative_that_got_further() {
    // The path got to its property map's NULL entry (E118 at the entry); the parenthesised expression stopped at `{`.
    assert_eq!(
        at("MATCH (n) WHERE (n {title: null})-->(m) RETURN n"),
        (E118, 1, 21)
    );
    // The parenthesised expression got further.
    assert_eq!(at("MATCH (n) WHERE (n.x + ) RETURN n"), (E001, 1, 24));
    assert!(
        parse_read(
            "MATCH (n) WHERE (n.x) > 1 RETURN n",
            ParseOptions::default()
        )
        .is_ok()
    );
}

/// [LQ/lexical §7.6], [LQ/errors §5.2]: whitespace or a comment ends a revision; a suffix after them is E001 with the
/// text of the table, at the suffix.
#[test]
fn a_suffix_after_whitespace_or_a_comment() {
    for (src, col) in [
        ("USE main /* x */ ~2 MATCH (t) RETURN t", 18),
        ("USE HEAD ^ MATCH (t) RETURN t", 10),
        ("CALL log(main..lane/x @1) YIELD commit RETURN commit", 23),
    ] {
        assert_eq!(at(src), (E001, 1, col), "{src}");
        let e = parse_read(src, ParseOptions::default()).unwrap_err();
        assert_eq!(e[0].message, "a revision ends at whitespace or a comment");
    }
    assert_eq!(
        first_err_tx("TX ON lane/x ~1 { DELETE #1 }", false),
        (E001, 1, 14)
    );
}
