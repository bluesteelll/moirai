//! Every production of grammar v1 ([LQ/grammar-v1.ebnf] §1–§6), checked through the S-expression form of
//! [LQ/canonical-ast §4], and the token-stream examples of [LQ/lexical §11.2].

use super::{assert_sx, sx, sx_define, sx_tx};
use crate::lq::parser::{ParseOptions, parse_read, parse_write, token_stream_text};

const N: &str = "(npat _ [] [] _)";

fn part(clauses: &str, ret: &str) -> String {
    format!("(read run (query [(part _ [{clauses}] {ret} _)] []))")
}

fn ret_n(var: &str) -> String {
    format!("(return false false [(item (ident \"{var}\") _)] [] [] _)")
}

fn m(patterns: &str, where_: &str) -> String {
    format!("(match false _ [{patterns}] {where_})")
}

fn np(var: &str) -> String {
    format!("(npat \"{var}\" [] [] _)")
}

/// The expression of `RETURN <e>` after `MATCH (n)`.
fn e(src: &str) -> String {
    let full = sx(&format!("MATCH (n) RETURN {src}"));
    let start = full.find("(item ").expect("an item") + 6;
    let end = full
        .rfind(" _)] [] [] _) _)] []))")
        .expect("the item's end");
    full[start..end].to_string()
}

#[test]
fn canonical_ast_example_4_4() {
    assert_sx(
        &sx(
            "MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5",
        ),
        r#"(read run
             (query
              [(part _
                [(match false _
                  [(path (npat "t" ["task"] [] _) [])]
                  (and (cmp = (prop (ident "t") "status") (str "open"))
                       (cmp <= (prop (ident "t") "priority") (int 1))))]
                (return false false [(item (ident "t") _)] [] [(sort (prop (ident "t") "priority") asc)] (int 5))
                _)]
              []))"#,
    );
    // The second text of §4.4: names as written, a bare word and a string where the first had others.
    assert_sx(
        &sx(
            "match (x:Task) where x.status = open and x.priority <= 'P1' return x order by x.priority asc limit 5",
        ),
        r#"(read run (query [(part _ [(match false _ [(path (npat "x" ["Task"] [] _) [])]
             (and (cmp = (prop (ident "x") "status") (ident "open")) (cmp <= (prop (ident "x") "priority") (str "P1"))))]
             (return false false [(item (ident "x") _)] [] [(sort (prop (ident "x") "priority") asc)] (int 5)) _)] []))"#,
    );
}

#[test]
fn entry_points_and_set_operations() {
    let s = sx("EXPLAIN MATCH (n) RETURN n");
    assert!(s.starts_with("(read explain "), "{s}");
    assert!(sx("profile MATCH (n) RETURN n").starts_with("(read profile "));
    let s = sx(
        "MATCH (a) RETURN a UNION MATCH (b) RETURN b UNION ALL MATCH (c) RETURN c EXCEPT MATCH (d) RETURN d INTERSECT MATCH (e) RETURN e",
    );
    assert!(s.ends_with("[union union_all except intersect]))"), "{s}");
    assert_eq!(s.matches("(part _").count(), 5);
}

#[test]
fn use_and_revisions() {
    assert_sx(
        &sx("USE main~2 MATCH (n) RETURN n"),
        &format!(
            "(read run (query [(part (rsuf (rref \"main\") tilde 2 _) [{}] {} _)] []))",
            m(&format!("(path {} [])", np("n")), "_"),
            ret_n("n")
        ),
    );
    let s = sx("USE lane/l5np^ MATCH (n) RETURN n");
    assert!(s.contains("(rsuf (rref \"lane/l5np\") caret 1 _)"), "{s}");
    let s = sx("USE main@3 MATCH (n) RETURN n");
    assert!(s.contains("(rsuf (rref \"main\") at 3 _)"), "{s}");
    let s = sx("USE main@{3} MATCH (n) RETURN n");
    assert!(s.contains("(rsuf (rref \"main\") at 3 _)"), "{s}");
    let s = sx("USE main@2026-09-25T10:00Z MATCH (n) RETURN n");
    assert!(
        s.contains("(rsuf (rref \"main\") attime _ \"2026-09-25T10:00:00Z\")"),
        "{s}"
    );
    let s = sx("USE c9b2e6c1 MATCH (n) RETURN n");
    assert!(s.contains("(rcommit \"9b2e6c1\")"), "{s}");
    let s = sx("USE s4466~1^2 MATCH (n) RETURN n");
    assert!(
        s.contains("(rsuf (rsuf (rseq 4466) tilde 1 _) caret 2 _)"),
        "{s}"
    );
    let s = sx("USE HEAD MATCH (n) RETURN n");
    assert!(s.contains("(part (rhead)"), "{s}");
    let s = sx("USE $r MATCH (n) RETURN n");
    assert!(s.contains("(part (param \"r\")"), "{s}");
    let s = sx("USE tags/v1.2 MATCH (n) RETURN n");
    assert!(s.contains("(rref \"tags/v1.2\")"), "{s}");
    // `s1` and `c9b2e6c1` are words outside revision positions (fixtures var-s1, var-hexlike).
    let s = sx("MATCH (s1:doc) WHERE s1.rev = c9b2e6c1 RETURN s1");
    assert!(
        s.contains("(ident \"c9b2e6c1\")") && s.contains("(npat \"s1\" [\"doc\"] [] _)"),
        "{s}"
    );
}

#[test]
fn revision_arguments_of_the_six_relations() {
    let s = sx("CALL log(main..lane/l5np) YIELD commit, actor, message");
    assert!(
        s.contains("(arg _ (rrange (rref \"main\") two (rref \"lane/l5np\")))"),
        "{s}"
    );
    let s = sx("CALL diff(HEAD...main, scope: #88)");
    assert!(
        s.contains("(arg _ (rrange (rhead) three (rref \"main\"))) (arg \"scope\" (nid 88))"),
        "{s}"
    );
    let s = sx("CALL diff(range: s4400 .. s4480)");
    assert!(
        s.contains("(arg \"range\" (rrange (rseq 4400) two (rseq 4480)))"),
        "{s}"
    );
    let s = sx("CALL changes(since: s4400, ref: lane/x) YIELD seq");
    assert!(
        s.contains("(arg \"since\" (rseq 4400)) (arg \"ref\" (rref \"lane/x\"))"),
        "{s}"
    );
    let s = sx("CALL history(#12, in: main..lane/x) YIELD seq");
    assert!(
        s.contains("(arg _ (nid 12)) (arg \"in\" (rrange (rref \"main\") two (rref \"lane/x\")))"),
        "{s}"
    );
    let s = sx("CALL across(refs: [main, lane/x], ids: [#12]) YIELD node");
    assert!(
        s.contains("(arg \"refs\" (rlist [(rref \"main\") (rref \"lane/x\")]))"),
        "{s}"
    );
    let s = sx("USE merge/main/from/lane/l10 CALL violations()");
    assert!(s.contains("(scall \"violations\" [] none"), "{s}");
    let s = sx("CALL violations(ref: merge/main/from/lane/x)");
    assert!(
        s.contains("(arg \"ref\" (rref \"merge/main/from/lane/x\"))"),
        "{s}"
    );
    // A quote at an argument position reads an expression; a parameter is a revision parameter.
    let s = sx("CALL diff('main..x')");
    assert!(s.contains("(arg _ (str \"main..x\"))"), "{s}");
    let s = sx("CALL log($r, actor: 'dev#1')");
    assert!(
        s.contains("(arg _ (param \"r\")) (arg \"actor\" (str \"dev#1\"))"),
        "{s}"
    );
    // std.diff and named queries are not revision positions: the word is an identifier.
    let s = sx("CALL std.diff(range: main)");
    assert!(s.contains("(arg \"range\" (ident \"main\"))"), "{s}");
}

#[test]
fn match_modes_and_optional_match() {
    for (mode, atom) in [
        ("WALK", "walk"),
        ("trail", "trail"),
        ("ACYCLIC", "acyclic"),
        ("SIMPLE", "simple"),
    ] {
        let s = sx(&format!("MATCH {mode} (a)-->(b) RETURN b"));
        assert!(s.contains(&format!("(match false {atom} ")), "{s}");
    }
    assert!(sx("MATCH DIFFERENT RELATIONSHIPS (a) RETURN a").contains("(match false different "));
    assert!(sx("MATCH DIFFERENT EDGES (a) RETURN a").contains("(match false different "));
    assert_sx(
        &sx("MATCH (t) OPTIONAL MATCH (t)<-[:BLOCKS]-(q) WHERE q.done RETURN t"),
        &part(
            &format!(
                "{} (match true _ [(path {} [(estep (epat _ left [\"BLOCKS\"] _ [] _) {})])] (prop (ident \"q\") \"done\"))",
                m(&format!("(path {} [])", np("t")), "_"),
                np("t"),
                np("q")
            ),
            &ret_n("t"),
        ),
    );
}

#[test]
fn call_clause_and_standalone_call() {
    assert_sx(
        &sx(
            "MATCH (t:task) CALL blockers(t, transitive: true) YIELD blocker AS b, depth WHERE depth > 1 RETURN t",
        ),
        &part(
            &format!(
                "{} (call \"blockers\" [(arg _ (ident \"t\")) (arg \"transitive\" (bool true))] [(yitem \"blocker\" \"b\") (yitem \"depth\" _)] (cmp > (ident \"depth\") (int 1)))",
                m("(path (npat \"t\" [\"task\"] [] _) [])", "_")
            ),
            &ret_n("t"),
        ),
    );
    assert_sx(
        &sx(
            "CALL search('lease', kinds: ['note']) YIELD * WHERE score > 1 ORDER BY score DESC LIMIT 5",
        ),
        r#"(read run (query [(part _ [] _ (scall "search" [(arg _ (str "lease")) (arg "kinds" (list [(str "note")]))] star []
             (cmp > (ident "score") (int 1)) [(sort (ident "score") desc)] (int 5)))] []))"#,
    );
    assert_sx(
        &sx("CALL std.ready(scope: #88)"),
        r#"(read run (query [(part _ [] _ (scall "std.ready" [(arg "scope" (nid 88))] none [] _ [] _))] []))"#,
    );
    let s = sx("CALL conflicts() YIELD key RETURN key");
    assert!(
        s.contains("(call \"conflicts\" [] [(yitem \"key\" _)] _)"),
        "{s}"
    );
    // Yield field names are plain names; `key`, `node` and `class` are no keywords there.
    let s = sx("CALL conflicts() YIELD key, node, class ORDER BY key LIMIT $n");
    assert!(s.contains("items [(yitem \"key\" _) (yitem \"node\" _) (yitem \"class\" _)] _ [(sort (ident \"key\") asc)] (param \"n\")"), "{s}");
}

#[test]
fn unwind_with_and_return() {
    assert_sx(
        &sx("UNWIND [1, 2] AS x RETURN x"),
        &part("(unwind (list [(int 1) (int 2)]) \"x\")", &ret_n("x")),
    );
    assert_sx(
        &sx(
            "MATCH (t:task) WITH DISTINCT t.assignee AS a, count(*) AS n WHERE n > 1 ORDER BY n DESC LIMIT 3 RETURN a, n",
        ),
        &part(
            &format!(
                "{} (with true false [(item (prop (ident \"t\") \"assignee\") \"a\") (item (countstar) \"n\")] (cmp > (ident \"n\") (int 1)) [(sort (ident \"n\") desc)] (int 3))",
                m("(path (npat \"t\" [\"task\"] [] _) [])", "_")
            ),
            "(return false false [(item (ident \"a\") _) (item (ident \"n\") _)] [] [] _)",
        ),
    );
    assert_sx(
        &sx("MATCH (a) WITH *, a.x AS x RETURN DISTINCT x"),
        &part(
            &format!(
                "{} (with false true [(item (prop (ident \"a\") \"x\") \"x\")] _ [] _)",
                m(&format!("(path {} [])", np("a")), "_")
            ),
            "(return true false [(item (ident \"x\") _)] [] [] _)",
        ),
    );
    assert_sx(
        &sx(
            "MATCH (t) RETURN ALL t.kind AS k, count(*) GROUP BY t.kind ORDER BY k ASCENDING, count(*) DESCENDING LIMIT 2",
        ),
        &part(
            &m(&format!("(path {} [])", np("t")), "_"),
            "(return false false [(item (prop (ident \"t\") \"kind\") \"k\") (item (countstar) _)] [(prop (ident \"t\") \"kind\")] [(sort (ident \"k\") asc) (sort (countstar) desc)] (int 2))",
        ),
    );
    let s = sx("MATCH (a) RETURN *");
    assert!(s.contains("(return false true [] [] [] _)"), "{s}");
    // `RETURN all(...)` is the list predicate, not `RETURN ALL` (P9).
    let s = sx("MATCH (t) RETURN all(x IN t.labels WHERE x = 'a')");
    assert!(s.contains("(listpred all \"x\""), "{s}");
}

#[test]
fn node_patterns() {
    let s = sx("MATCH (#51) RETURN 1");
    assert!(s.contains("(npat _ [] [(kv \"id\" (nid 51))] _)"), "{s}");
    assert_eq!(sx("MATCH (#51) RETURN 1"), sx("MATCH ({id: #51}) RETURN 1"));
    let s = sx("MATCH (#u:018f3c2e7a117b3c9d5e4c2f1a0b9e77) RETURN 1");
    assert!(
        s.contains("(npat _ [] [(kv \"id\" (uid \"018f3c2e7a117b3c9d5e4c2f1a0b9e77\"))] _)"),
        "{s}"
    );
    let s = sx("MATCH (t:task|doc {id: 5, status: 'open'} WHERE t.x > 1) RETURN t");
    assert!(
        s.contains("(npat \"t\" [\"task\" \"doc\"] [(kv \"id\" (int 5)) (kv \"status\" (str \"open\"))] (cmp > (prop (ident \"t\") \"x\") (int 1)))"),
        "{s}"
    );
    assert!(sx("MATCH () RETURN 1").contains(N));
    assert!(sx("MATCH ({}) RETURN 1").contains(N));
    let s = sx("MATCH (`my var`:`MATCH`) RETURN `my var`");
    assert!(
        s.contains("(npat \"my var\" [\"MATCH\"] [] _)") && s.contains("(ident \"my var\")"),
        "{s}"
    );
    // Reserved words are plain names after ':' and '|'.
    let s = sx("MATCH (n:where|return) RETURN n");
    assert!(s.contains("[\"where\" \"return\"]"), "{s}");
}

#[test]
fn edge_patterns_and_quantifiers() {
    let edge = |p: &str| {
        let s = sx(&format!("MATCH (a){p}(b) RETURN b"));
        let i = s.find("(epat ").expect("an edge");
        let mut depth = 0;
        let mut end = i;
        for (k, c) in s[i..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        s[i..end].to_string()
    };
    assert_sx(
        &edge("-[e:BLOCKS]->"),
        "(epat \"e\" right [\"BLOCKS\"] _ [] _)",
    );
    assert_sx(
        &edge("<-[:BLOCKS|GATES]-"),
        "(epat _ left [\"BLOCKS\" \"GATES\"] _ [] _)",
    );
    assert_sx(&edge("-[:RELATES]-"), "(epat _ both [\"RELATES\"] _ [] _)");
    assert_sx(&edge("-->"), "(epat _ right [] _ [] _)");
    assert_sx(&edge("<--"), "(epat _ left [] _ [] _)");
    assert_sx(&edge("--"), "(epat _ both [] _ [] _)");
    assert_sx(&edge("-[]->"), "(epat _ right [] _ [] _)");
    assert_sx(&edge("-[:T*]->"), "(epat _ right [\"T\"] (quant 1 _) [] _)");
    assert_sx(
        &edge("-[:T*2]->"),
        "(epat _ right [\"T\"] (quant 2 2) [] _)",
    );
    assert_sx(
        &edge("-[:T*1..3]->"),
        "(epat _ right [\"T\"] (quant 1 3) [] _)",
    );
    assert_sx(
        &edge("-[:T*..3]->"),
        "(epat _ right [\"T\"] (quant 1 3) [] _)",
    );
    assert_sx(
        &edge("-[:T*2..]->"),
        "(epat _ right [\"T\"] (quant 2 _) [] _)",
    );
    assert_sx(
        &edge("-[:T*0..]->"),
        "(epat _ right [\"T\"] (quant 0 _) [] _)",
    );
    assert_sx(&edge("-[:T]->+"), "(epat _ right [\"T\"] (quant 1 _) [] _)");
    assert_sx(&edge("-[:T]->*"), "(epat _ right [\"T\"] (quant 0 _) [] _)");
    assert_sx(
        &edge("-[:T]->{1,3}"),
        "(epat _ right [\"T\"] (quant 1 3) [] _)",
    );
    assert_sx(
        &edge("-[:T]->{2,}"),
        "(epat _ right [\"T\"] (quant 2 _) [] _)",
    );
    assert_sx(
        &edge("-[:T]->{2}"),
        "(epat _ right [\"T\"] (quant 2 2) [] _)",
    );
    assert_sx(
        &edge("-[:T]->{,4}"),
        "(epat _ right [\"T\"] (quant 0 4) [] _)",
    );
    assert_sx(&edge("-->+"), "(epat _ right [] (quant 1 _) [] _)");
    assert_sx(
        &edge("-[e:T {pinned: 'c1'} WHERE e.flagged]->"),
        "(epat \"e\" right [\"T\"] _ [(kv \"pinned\" (str \"c1\"))] (prop (ident \"e\") \"flagged\"))",
    );
    // Both quantifier spellings are one S-AST (§3.1 item 4).
    assert_eq!(
        sx("MATCH (a)-[:T*1..3]->(b) RETURN b"),
        sx("MATCH (a)-[:T]->{1,3}(b) RETURN b")
    );
}

#[test]
fn group_patterns() {
    assert_sx(
        &sx("MATCH (x)((a)-[:BLOCKS]->(b) WHERE a.p = 1){1,3}(y) RETURN y"),
        &part(
            &m(
                &format!(
                    "(path {} [(gstep (group (path {} [(estep (epat _ right [\"BLOCKS\"] _ [] _) {})]) (cmp = (prop (ident \"a\") \"p\") (int 1)) (quant 1 3)) {})])",
                    np("x"),
                    np("a"),
                    np("b"),
                    np("y")
                ),
                "_",
            ),
            &ret_n("y"),
        ),
    );
    let s = sx("MATCH (x)((a)-->(b))+(y) RETURN y");
    assert!(s.contains("(quant 1 _)) (npat \"y\""), "{s}");
}

#[test]
fn expressions_by_precedence() {
    assert_sx(
        &e("1 OR 2 AND NOT 3"),
        "(or (int 1) (and (int 2) (not (int 3))))",
    );
    assert_sx(
        &e("n.a = 1 AND n.b <> 2"),
        "(and (cmp = (prop (ident \"n\") \"a\") (int 1)) (cmp <> (prop (ident \"n\") \"b\") (int 2)))",
    );
    assert_sx(
        &e("n.a != 1"),
        "(cmp <> (prop (ident \"n\") \"a\") (int 1))",
    );
    for (op, atom) in [("<", "<"), ("<=", "<="), (">", ">"), (">=", ">=")] {
        assert_sx(
            &e(&format!("1 {op} 2")),
            &format!("(cmp {atom} (int 1) (int 2))"),
        );
    }
    assert_sx(
        &e("n.a IS NULL"),
        "(isnull false (prop (ident \"n\") \"a\"))",
    );
    assert_sx(
        &e("n.a IS NOT NULL"),
        "(isnull true (prop (ident \"n\") \"a\"))",
    );
    assert_sx(&e("1 IN [1]"), "(in (int 1) (list [(int 1)]))");
    assert_sx(&e("1 NOT IN [1]"), "(not (in (int 1) (list [(int 1)])))");
    assert_sx(
        &e("'ab' STARTS WITH 'a'"),
        "(strpred starts (str \"ab\") (str \"a\"))",
    );
    assert_sx(
        &e("'ab' ENDS WITH 'b'"),
        "(strpred ends (str \"ab\") (str \"b\"))",
    );
    assert_sx(
        &e("'ab' CONTAINS 'b'"),
        "(strpred contains (str \"ab\") (str \"b\"))",
    );
    assert_sx(
        &e("n:note|rule"),
        "(labeltest (ident \"n\") [\"note\" \"rule\"])",
    );
    assert_sx(
        &e("1 + 2 * 3 - 4 / 5"),
        "(arith - (arith + (int 1) (arith * (int 2) (int 3))) (arith / (int 4) (int 5)))",
    );
    assert_sx(&e("-n.a"), "(neg (prop (ident \"n\") \"a\"))");
    assert_sx(
        &e("n.a - -1"),
        "(arith - (prop (ident \"n\") \"a\") (neg (int 1)))",
    );
    assert_sx(
        &e("(n.a) - 1"),
        "(arith - (prop (ident \"n\") \"a\") (int 1))",
    );
    assert_sx(&e("(n) - -1"), "(arith - (ident \"n\") (neg (int 1)))");
    assert_sx(&e("n.a.b"), "(prop (prop (ident \"n\") \"a\") \"b\")");
    assert_sx(&e("#133.body"), "(prop (nid 133) \"body\")");
    assert_sx(&e("(1 OR 2) AND 3"), "(and (or (int 1) (int 2)) (int 3))");
    assert_sx(&e("n.order"), "(prop (ident \"n\") \"order\")");
}

#[test]
fn literals_parameters_and_names() {
    assert_sx(&e("010"), "(int 10)");
    assert_sx(&e("1.50"), "(float \"1.50\")");
    assert_sx(&e("1e-3"), "(float \"1e-3\")");
    assert_sx(&e("3d"), "(dur \"3d\")");
    assert_sx(&e("'a\\'b'"), "(str \"a'b\")");
    assert_sx(&e("\"x\\u{41}\""), "(str \"xA\")");
    assert_sx(&e("TRUE"), "(bool true)");
    assert_sx(&e("false"), "(bool false)");
    assert_sx(&e("null"), "(null)");
    assert_sx(&e("$scope"), "(param \"scope\")");
    assert_sx(&e("#0040"), "(nid 40)");
    assert_sx(&e("`a b`"), "(ident \"a b\")");
    assert_sx(&e("key"), "(ident \"key\")");
}

#[test]
fn functions_and_their_keyword_forms() {
    assert_sx(&e("count(*)"), "(countstar)");
    assert_sx(
        &e("COUNT(DISTINCT n.a)"),
        "(fn \"COUNT\" true [(arg _ (prop (ident \"n\") \"a\"))])",
    );
    assert_sx(
        &e("toLower(n.title)"),
        "(fn \"toLower\" false [(arg _ (prop (ident \"n\") \"title\"))])",
    );
    assert_sx(
        &e("f(k: 1, (k:note))"),
        "(fn \"f\" false [(arg \"k\" (int 1)) (arg _ (labeltest (ident \"k\") [\"note\"]))])",
    );
    assert_sx(
        &e("any(x IN n.labels WHERE x = 'a')"),
        "(listpred any \"x\" (prop (ident \"n\") \"labels\") (cmp = (ident \"x\") (str \"a\")))",
    );
    assert_sx(
        &e("none(x IN [1] WHERE x > 1)"),
        "(listpred none \"x\" (list [(int 1)]) (cmp > (ident \"x\") (int 1)))",
    );
    assert_sx(
        &e("exists(n.p)"),
        "(isnull true (prop (ident \"n\") \"p\"))",
    );
    assert_sx(
        &e("exists((n)-->())"),
        &format!(
            "(exists (subp [(path {} [(estep (epat _ right [] _ [] _) {N})])] _))",
            np("n")
        ),
    );
    assert_sx(
        &e("size((n)<-[:BLOCKS]-())"),
        &format!(
            "(countsub (subp [(path {} [(estep (epat _ left [\"BLOCKS\"] _ [] _) {N})])] _))",
            np("n")
        ),
    );
    assert_sx(
        &e("size(n.labels)"),
        "(fn \"size\" false [(arg _ (prop (ident \"n\") \"labels\"))])",
    );
    assert_sx(
        &e("exists(n.p, 1)"),
        "(fn \"exists\" false [(arg _ (prop (ident \"n\") \"p\")) (arg _ (int 1))])",
    );
    assert_sx(&e("`all`(1)"), "(fn \"all\" false [(arg _ (int 1))])");
    assert_sx(&e("datetime()"), "(fn \"datetime\" false [])");
}

#[test]
fn subqueries_and_pattern_predicates() {
    assert_sx(
        &e("(n)<-[:BLOCKS]-()"),
        &format!(
            "(exists (subp [(path {} [(estep (epat _ left [\"BLOCKS\"] _ [] _) {N})])] _))",
            np("n")
        ),
    );
    assert_sx(
        &e("NOT (n)-->()"),
        &format!(
            "(not (exists (subp [(path {} [(estep (epat _ right [] _ [] _) {N})])] _)))",
            np("n")
        ),
    );
    assert_sx(
        &e("EXISTS { (n)-->(m) WHERE m.x }"),
        &format!(
            "(exists (subp [(path {} [(estep (epat _ right [] _ [] _) {})])] (prop (ident \"m\") \"x\")))",
            np("n"),
            np("m")
        ),
    );
    assert_sx(
        &e("COUNT { MATCH (n)-->(m) RETURN m }"),
        &format!(
            "(countsub (subq [(match false _ [(path {} [(estep (epat _ right [] _ [] _) {})])] _)] (return false false [(item (ident \"m\") _)] [] [] _)))",
            np("n"),
            np("m")
        ),
    );
    assert_sx(
        &e("EXISTS { MATCH (n)-->(m) }"),
        &format!(
            "(exists (subq [(match false _ [(path {} [(estep (epat _ right [] _ [] _) {})])] _)] _))",
            np("n"),
            np("m")
        ),
    );
    // A group after the first node pattern starts a path too (P5).
    let s = e("(n)((a)-->(b)){1,2}(m)");
    assert!(s.starts_with("(exists (subp [(path (npat \"n\""), "{s}");
}

#[test]
fn lists_maps_and_case() {
    assert_sx(&e("[]"), "(list [])");
    assert_sx(&e("{}"), "(map [])");
    assert_sx(
        &e("{a: 1, `b c`: 'x'}"),
        "(map [(kv \"a\" (int 1)) (kv \"b c\" (str \"x\"))])",
    );
    assert_sx(
        &e("CASE WHEN n.a THEN 1 ELSE 2 END"),
        "(case _ [(when (prop (ident \"n\") \"a\") (int 1))] (int 2))",
    );
    assert_sx(
        &e("CASE n.s WHEN 'a' THEN 1 WHEN 'b' THEN 2 END"),
        "(case (prop (ident \"n\") \"s\") [(when (str \"a\") (int 1)) (when (str \"b\") (int 2))] _)",
    );
}

#[test]
fn transactions() {
    assert_sx(
        &sx_tx(
            "TX ON lane/l5np LEASE 'L-18' { MATCH (t {id: #89}) WHERE t.status = 'in_progress' EXPECT 1 SET t.done = true }",
        ),
        r#"(tx (rref "lane/l5np") _ _ _ "L-18" _
             [(smatch [(path (npat "t" [] [(kv "id" (nid 89))] _) [])] (cmp = (prop (ident "t") "status") (str "in_progress"))
               (expect exact 1 _ _) [(mset [(assign (ident "t") "done" (bool true))])])]
             false)"#,
    );
    assert_sx(
        &sx_tx(
            "TX MESSAGE 'm' IF TARGETS 'ab' KEY 'k' IF TIP c9b2e6c1 { SET #1.a = 1, #2.b = $v; } DRY",
        ),
        r#"(tx _ (rcommit "9b2e6c1") "ab" "k" _ "m"
             [(smuts [(mset [(assign (nid 1) "a" (int 1)) (assign (nid 2) "b" (param "v"))])])] true)"#,
    );
    let s = sx_tx(
        "TX { REMOVE t.a, $x.b; DELETE a, #2 POLICY CASCADE REPLACED BY #3 RELEASE REASON 'dup' }",
    );
    assert!(
        s.contains("(smuts [(mremove [(tprop (ident \"t\") \"a\") (tprop (param \"x\") \"b\")])])"),
        "{s}"
    );
    assert!(
        s.contains("(mdelete [(ident \"a\") (nid 2)] [(dpolicy cascade) (dreplaced (nid 3)) (drelease) (dreason (str \"dup\"))])"),
        "{s}"
    );
    let s = sx_tx("TX { MOVE #1 UNDER #2 BEFORE #3; MOVE #1 UNDER #2 FIRST; MOVE a UNDER b }");
    assert!(s.contains("(mmove (nid 1) (nid 2) before (nid 3))"), "{s}");
    assert!(
        s.contains("(mmove (nid 1) (nid 2) first _)")
            && s.contains("(mmove (ident \"a\") (ident \"b\") _ _)"),
        "{s}"
    );
    let s = sx_tx("TX { CREATE (#12)-[:BLOCKS]->(#51); INSERT (a)<-[:CITES {pinned: 'c'}]-(b) }");
    assert!(
        s.contains("(medge (nid 12) right \"BLOCKS\" [] (nid 51))"),
        "{s}"
    );
    assert!(
        s.contains(
            "(medge (ident \"a\") left \"CITES\" [(kv \"pinned\" (str \"c\"))] (ident \"b\"))"
        ),
        "{s}"
    );
    let s = sx_tx("TX { REOPEN #9 REASON 'again'; PATCH #133.body REMOVE $old ADD $new }");
    assert!(
        s.contains("(mreopen (nid 9) (str \"again\"))")
            && s.contains("(mpatch (nid 133) \"body\" (param \"old\") (param \"new\"))"),
        "{s}"
    );
    let s = sx_tx(
        "TX { CREATE (n:task {title: $t})-[:BLOCKS]->(#12)<-[:CHILD_OF]-(#13) UNDER #88 UNLESS EXISTS { (n:task {title: $t}) } }",
    );
    assert!(
        s.contains(
            "(screate \"n\" \"task\" [(kv \"title\" (param \"t\"))] [(cedge right \"BLOCKS\" [] (nid 12)) (cedge left \"CHILD_OF\" [] (nid 13))] (nid 88) (subp [(path (npat \"n\" [\"task\"] [(kv \"title\" (param \"t\"))] _) [])] _))"
        ),
        "{s}"
    );
    let s = sx_tx("TX { INSERT (f:finding) }");
    assert!(s.contains("(screate \"f\" \"finding\" [] [] _ _)"), "{s}");
    let s = sx_tx("TX LEASE 'L-18' { CALL tx.complete(#89, outcome: 'done') YIELD ready }");
    assert!(s.contains("(stxcall \"complete\" [(arg _ (nid 89)) (arg \"outcome\" (str \"done\"))] [(yitem \"ready\" _)])"), "{s}");
    let s = sx_tx("TX { ASSERT 1 = 1 ELSE 'no'; ASSERT true }");
    assert!(
        s.contains("(sassert (cmp = (int 1) (int 1)) \"no\") (sassert (bool true) _)"),
        "{s}"
    );
    let s = sx_tx(
        "TX { RESOLVE '#91.body' TAKE OURS; RESOLVE (CALL conflicts() YIELD key RETURN key) EXPECT >= 1 TAKE THEIRS }",
    );
    assert!(s.contains("(sresolve \"#91.body\" _ _ ours _ _)"), "{s}");
    assert!(
        s.contains("(sresolve _ (query [(part _ [(call \"conflicts\" [] [(yitem \"key\" _)] _)]"),
        "{s}"
    );
    assert!(s.contains("(expect ge 1 _ _) theirs _ _)"), "{s}");
    let s = sx_tx(
        "TX { RESOLVE 'k' TAKE VALUE $text; RESOLVE 'e' TAKE REPOINT #52; RESOLVE 'b' TAKE BASE }",
    );
    assert!(
        s.contains("(sresolve \"k\" _ _ value (param \"text\") _)")
            && s.contains("(sresolve \"e\" _ _ repoint _ (nid 52))"),
        "{s}"
    );
    let s =
        sx_tx("TX { DROP QUERY team.stale; DEFINE QUERY q($a: int) AS { MATCH (n) RETURN n } }");
    assert!(
        s.contains("(sdrop \"team.stale\")")
            && s.contains("(define \"q\" [(pdecl \"a\" (ptype \"int\" _) false _)] _ _"),
        "{s}"
    );
    let s = sx_tx(
        "TX { MATCH (t:task) EXPECT 2..5 SET t.a = 1; MATCH (t) EXPECT <= 3 DELETE t; MATCH (t) EXPECT $n SET t.a = 2 REMOVE t.b }",
    );
    assert!(
        s.contains("(expect range 2 5 _)")
            && s.contains("(expect le 3 _ _)")
            && s.contains("(expect param _ _ \"n\")"),
        "{s}"
    );
    assert!(s.contains("[(mset [(assign (ident \"t\") \"a\" (int 2))]) (mremove [(tprop (ident \"t\") \"b\")])]"), "{s}");
    // One ';' may precede '}'.
    assert_eq!(sx_tx("TX { SET #1.a = 1; }"), sx_tx("TX { SET #1.a = 1 }"));
}

#[test]
fn definitions() {
    assert_sx(
        &sx_define(
            "DEFINE QUERY ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node BUDGET light AS {\n  MATCH (t:task)\n  WHERE t.ready\n  RETURN t ORDER BY t.priority LIMIT $limit\n}",
        ),
        r#"(define "ready"
             [(pdecl "scope" (ptype "node" _) true (null)) (pdecl "role" (ptype "text" _) true (null)) (pdecl "limit" (ptype "int" _) false (int 20))]
             "node" "light"
             (query [(part _ [(match false _ [(path (npat "t" ["task"] [] _) [])] (prop (ident "t") "ready"))]
               (return false false [(item (ident "t") _)] [] [(sort (prop (ident "t") "priority") asc)] (param "limit")) _)] []))"#,
    );
    let s = sx_define(
        "DEFINE QUERY a.b($ids: list<node>, $p: range<int>? = NULL, $d: duration = 3d, $n: node = #40) AS { MATCH (n) RETURN n }",
    );
    assert!(
        s.contains("(pdecl \"ids\" (ptype \"list\" \"node\") false _)"),
        "{s}"
    );
    assert!(
        s.contains("(pdecl \"p\" (ptype \"range\" \"int\") true (null))"),
        "{s}"
    );
    assert!(
        s.contains("(pdecl \"d\" (ptype \"duration\" _) false (dur \"3d\"))")
            && s.contains("(nid 40)"),
        "{s}"
    );
}

#[test]
fn comments_whitespace_and_case() {
    assert_eq!(
        sx("match (t:task) // c\n where /* x */ t.a = 1\r\n return t"),
        sx("MATCH (t:task) WHERE t.a = 1 RETURN t")
    );
    // A BOM is stripped once by the byte entry point.
    let p = crate::lq::parser::parse_read_bytes(
        b"\xEF\xBB\xBFMATCH (n) RETURN n",
        ParseOptions::default(),
    )
    .unwrap();
    assert!(crate::lq::sexpr::read(&p.tree).starts_with("(read run"));
}

fn stream(src: &str) -> String {
    token_stream_text(&parse_read(src, ParseOptions::default()).unwrap().tokens)
}

#[test]
fn token_stream_examples_of_lexical_11_2() {
    assert_eq!(
        stream("CALL log(main..lane/l5np) YIELD commit, actor, message"),
        "KW CALL\nNAME log\nP (\nREF main\nRANGE ..\nREF lane/l5np\nP )\nKW YIELD\nNAME commit\nP ,\nNAME actor\nP ,\nNAME message\nEOF -\n"
    );
    assert_eq!(
        stream("USE main@2026-09-25T10:00Z MATCH (s1:doc) WHERE s1 IN subtree(#130) RETURN s1"),
        "KW USE\nREF main\nSUF @2026-09-25T10:00:00Z\nKW MATCH\nP (\nNAME s1\nP :\nNAME doc\nP )\nKW WHERE\nNAME s1\nKW IN\nNAME subtree\nP (\nNODE 130\nP )\nKW RETURN\nNAME s1\nEOF -\n"
    );
    let p = parse_write(
        "TX ON lane/l5np LEASE 'L-18' { MATCH (t {id: #89}) WHERE t.status = 'in_progress' EXPECT 1 SET t.done = true }",
        ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        token_stream_text(&p.tokens),
        "KW TX\nKW ON\nREF lane/l5np\nKW LEASE\nSTR \"L-18\"\nP {\nKW MATCH\nP (\nNAME t\nP {\nNAME id\nP :\nNODE 89\nP }\nP )\nKW WHERE\nNAME t\nP .\nNAME status\nP =\nSTR \"in_progress\"\nKW EXPECT\nINT 1\nKW SET\nNAME t\nP .\nNAME done\nP =\nKW TRUE\nP }\nEOF -\n"
    );
}

#[test]
fn token_stream_classifies_keyword_forms() {
    let s = stream(
        "MATCH (n) RETURN count(*), count(n), `x`, all(v IN n.l WHERE v = 1), exists((n)-->())",
    );
    assert!(s.contains("KW COUNT\nP (\nP *\nP )"), "{s}");
    assert!(s.contains("NAME count\nP (\nNAME n\nP )"), "{s}");
    assert!(s.contains("QNAME \"x\""), "{s}");
    assert!(s.contains("KW ALL\nP (\nNAME v\nKW IN"), "{s}");
    assert!(s.contains("KW EXISTS\nP (\nP (\nNAME n"), "{s}");
    // `EXISTS` is reserved, so `exists( e )` is a keyword form too; `size( e )` is a generic call.
    let s = stream("MATCH (t) RETURN exists(t.a), size(t.l)");
    assert!(s.contains("KW EXISTS\nP (\nNAME t\nP .\nNAME a"), "{s}");
    assert!(s.contains("NAME size\nP (\nNAME t"), "{s}");
    // The `tx` of a `tx_name` is a plain name ([LQ/grammar-v1.ebnf §P.3]: every `tx_name` segment).
    let p = parse_write(
        "TX { CALL tx.complete(#89, outcome: 'done', summary: 's') }",
        ParseOptions::default(),
    )
    .unwrap();
    assert!(
        token_stream_text(&p.tokens).contains("KW CALL\nNAME tx\nP .\nNAME complete\nP ("),
        "{}",
        token_stream_text(&p.tokens)
    );
    let s = stream("CALL diff(HEAD~2...main)");
    assert!(s.contains("HEAD HEAD\nSUF ~2\nRANGE ...\nREF main"), "{s}");
    let s = stream("CALL across(refs: [main, lane/x], ids: [#1])");
    assert!(
        s.contains("NAME refs\nP :\nP [\nREF main\nP ,\nREF lane/x\nP ]\nP ,\nNAME ids"),
        "{s}"
    );
    let s = stream("CALL diff(s12..c0123456)");
    assert!(s.contains("SEQ 12\nRANGE ..\nCOMMIT 0123456"), "{s}");
    let s = stream("MATCH (n) WHERE n.d > 1.5 AND n.e < 3d RETURN $p");
    assert!(
        s.contains("FLOAT 1.5") && s.contains("DUR 3d") && s.contains("PARAM p"),
        "{s}"
    );
}
