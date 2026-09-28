//! The binder ([LQ/canonical-ast §5]): the worked example of §4.4/§6.5, the normalisations and scope rules that make
//! spellings and renamings hash alike, every code the binder raises ([LQ/errors §5.1] column M0 = `model`, raised by
//! the binder or the tx binder), the lints W01, W02, W07, W10 and N08, the reading echo of [LQ/envelope §4] and the
//! portable form of definitions (§8).

use super::fixture::{
    self, bytes_of, cast_of, read, read_err, read_err_with, write, write_err, write_err_with,
};
use crate::lq::bind::LintKind;
use crate::lq::cast::{Root, encode, sexpr};
use crate::lq::ctx::{Caller, Params, Profile, Surface, Value};
use crate::lq::diag::Code;
use crate::lq::printer::Spelling;
use crate::lq::sexpr::same;

// ----- the canonical form -------------------------------------------------------------------------------------------

/// [LQ/canonical-ast §4.4]: two spellings of one query give the C-AST printed there and the bytes of §6.5.
#[test]
fn worked_example_of_4_4_and_6_5() {
    let a = "MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5";
    let b = "match (x:Task) where x.status = open and x.priority <= 'P1' return x order by x.priority asc limit 5";
    let want = r#"(QUERY
 (PART _
  (CLAUSES
   [(MATCH false
     [(PATH (NODEP 0 ["task"] [] _) [])]
     (AND (CMP = (PROP (VAR 0) "status") (ENUM "open"))
          (CMP <= (PROP (VAR 0) "priority") (INT 1))))]
   (RETURN false false [(RITEM (VAR 0) _)] [(SORT (PROP (VAR 0) "priority") false)] (INT 5))))
 [])"#;
    assert!(same(&cast_of(a), want), "{}", cast_of(a));
    assert!(same(&cast_of(b), want), "{}", cast_of(b));
    let bytes = bytes_of(a);
    assert_eq!(bytes.len(), 174);
    assert_eq!(bytes, bytes_of(b));
    assert_eq!(&bytes[..22], b"\x10\x00\x00\x00moirai-lq-ast-v1\x01\x00");
}

fn same_cast(a: &str, b: &str) {
    assert_eq!(cast_of(a), cast_of(b), "\n{a}\n{b}");
    assert_eq!(bytes_of(a), bytes_of(b));
}

#[test]
fn spellings_and_names_do_not_change_the_hash() {
    // Variable names (§5.7), keyword case, formatting and comments.
    same_cast(
        "MATCH (a)-[:BLOCKS]->(b) RETURN a, b",
        "match (x) -[:blocks]-> (y) /* c */ return x, y",
    );
    // Reverse aliases and stored names (§5.4).
    same_cast(
        "MATCH (#51)<-[:BLOCKS]-(b) RETURN b",
        "MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b",
    );
    same_cast(
        "MATCH (a)-[:DERIVED_FROM]->(b) RETURN a",
        "MATCH (a)-[:derived_from]->(b) RETURN a",
    );
    same_cast(
        "MATCH (a)-[:SUBTASK_OF]->(b) RETURN a",
        "MATCH (a)-[:CHILD_OF]->(b) RETURN a",
    );
    // Quantifier spellings (§3.1 item 4).
    same_cast(
        "MATCH (a)-[:BLOCKS*1..3]->(b) RETURN b",
        "MATCH (a)-[:BLOCKS]->{1,3}(b) RETURN b",
    );
    same_cast(
        "MATCH (a)-[:BLOCKS*]->(b) RETURN b",
        "MATCH (a)-[:BLOCKS]->+(b) RETURN b",
    );
    // `!=`, NOT IN, IS NOT NULL, exists(), pattern predicates, node literals (§3.1).
    same_cast(
        "MATCH (t) WHERE t.assignee != 'x' RETURN t",
        "MATCH (t) WHERE t.assignee <> 'x' RETURN t",
    );
    same_cast(
        "MATCH (t) WHERE exists(t.assignee) RETURN t",
        "MATCH (t) WHERE t.assignee IS NOT NULL RETURN t",
    );
    same_cast(
        "MATCH (t) WHERE (t)-[:BLOCKS]->() RETURN t",
        "MATCH (t) WHERE EXISTS { (t)-[:BLOCKS]->() } RETURN t",
    );
    same_cast(
        "MATCH (#51)-[:BLOCKS]->(b) RETURN b",
        "MATCH ({id: #51})-[:BLOCKS]->(b) RETURN b",
    );
    // EXPLAIN is a mode (§5.2); match modes are dropped.
    same_cast(
        "EXPLAIN MATCH (t:task) RETURN t",
        "MATCH TRAIL (t:task) RETURN t",
    );
    // N1: a clause subquery that is one MATCH is its pattern form.
    same_cast(
        "MATCH (t) WHERE EXISTS { MATCH (t)-[:BLOCKS]->(x) } RETURN t",
        "MATCH (t) WHERE EXISTS { (t)-[:BLOCKS]->(x) } RETURN t",
    );
    // GROUP BY is checked, then dropped.
    same_cast(
        "MATCH (t:task) RETURN t.status, count(*) GROUP BY t.status",
        "MATCH (t:task) RETURN t.status, count(*)",
    );
    // A priority written three ways.
    same_cast(
        "MATCH (t:task) WHERE t.priority = 1 RETURN t",
        "MATCH (t:task) WHERE t.priority = P1 RETURN t",
    );
}

#[test]
fn parameters_are_substituted_with_their_use_site_types() {
    let p = Params::new()
        .with("s", Value::Text("open".into()))
        .with("n", Value::Int(51))
        .with("ids", Value::Text("12,51".into()));
    let bound = fixture::read_with(
        "MATCH (t:task) WHERE t.status = $s AND t IN $ids AND t.id <> $n RETURN t",
        &p,
        &Caller::default(),
    )
    .unwrap();
    let lit =
        read("MATCH (t:task) WHERE t.status = 'open' AND t IN [#12, #51] AND t.id <> #51 RETURN t");
    assert_eq!(
        encode(Root::Query(&bound.ast)),
        encode(Root::Query(&lit.ast))
    );
    let p = Params::new().with("r", Value::Text("..1".into()));
    let b = fixture::read_with(
        "MATCH (t:task) WHERE t.priority IN $r RETURN t",
        &p,
        &Caller::default(),
    )
    .unwrap();
    assert!(sexpr(Root::Query(&b.ast)).contains("(RANGEINT _ 1)"));
}

#[test]
fn revisions_resolve_store_local_forms_only() {
    let c = cast_of("USE main~5 MATCH (t:task) RETURN t");
    assert!(c.contains("(RSUF (RREF \"main\") tilde 5)"), "{c}");
    let c = cast_of("USE s4466 MATCH (t:task) RETURN t");
    assert!(c.contains("(RCOMMIT \"41d7e0b2"), "{c}");
    same_cast(
        "USE s4466 MATCH (t:task) RETURN t",
        "USE c41d7e0b MATCH (t:task) RETURN t",
    );
    let c = cast_of("MATCH (t:task) WHERE t.rev = 4466 RETURN t");
    assert!(c.contains("(RCOMMIT \"41d7e0b2"), "{c}");
    same_cast(
        "MATCH (t:task) WHERE t.rev = 4466 RETURN t",
        "MATCH (t:task) WHERE t.rev = 's4466' RETURN t",
    );
    let c = cast_of("CALL diff(main...lane/x, scope: #88) YIELD node RETURN node");
    assert!(
        c.contains("(RRANGE (RREF \"main\") three (RREF \"lane/x\"))"),
        "{c}"
    );
    same_cast(
        "CALL diff(main..lane/x) YIELD node RETURN node",
        "CALL diff('main..lane/x') YIELD node RETURN node",
    );
}

/// N2: arguments of relations, named queries and named mutations are named and ordered by the callee's signature.
#[test]
fn arguments_follow_the_signature() {
    same_cast(
        "CALL blockers(#51, transitive: true) YIELD blocker RETURN blocker",
        "CALL blockers(transitive: true, n: #51) YIELD blocker RETURN blocker",
    );
    let c = cast_of("CALL ready(scope: 88) YIELD t RETURN t");
    assert!(
        c.contains("(CALL \"std.ready\" [(ARG \"scope\" (NODE"),
        "{c}"
    );
    let t = write("TX { CALL tx.complete(#89, summary: 's', outcome: 'done') }");
    let s = sexpr(Root::Tx(&t.ast));
    assert!(
        s.contains("[(ARG \"id\" (NODE") && s.find("\"outcome\"") < s.find("\"summary\""),
        "{s}"
    );
}

#[test]
fn scope_rules_number_variables() {
    // §5.7's example: a 0, b 1, x 2, c 3.
    let c = cast_of("MATCH (a)-[:BLOCKS]->(b) WITH b AS x MATCH (x)<-[:CHILD_OF]-(c) RETURN c");
    assert!(
        c.contains("(WITEM (VAR 1) 2)") && c.contains("(RITEM (VAR 3) _)"),
        "{c}"
    );
    // V4: an unaliased bare variable re-exports its binding.
    let c = cast_of("MATCH (t:task) WITH t RETURN t");
    assert!(c.contains("(WITEM (VAR 0) 0)"), "{c}");
    // V5: ORDER BY an alias of the same RETURN is ITEMREF.
    let c = cast_of("MATCH (t:task) RETURN t.priority AS p, t ORDER BY p, t.id");
    assert!(
        c.contains("(SORT (ITEMREF 0) false)") && c.contains("(SORT (PROP (VAR 0) \"id\") false)"),
        "{c}"
    );
    // V8: the UNLESS EXISTS binding of the created variable's name is the created variable.
    let t = write(
        "TX { CREATE (f:finding {title: 'x', failure_scenario: 'y'}) UNLESS EXISTS { MATCH (f:finding) WHERE f.title = 'x' } }",
    );
    let s = sexpr(Root::Tx(&t.ast));
    assert!(
        s.contains("(SCREATE 0 \"finding\"") && s.contains("(NODEP 0 [\"finding\"]"),
        "{s}"
    );
    // V9: a list-predicate variable.
    let c =
        cast_of("MATCH (t:task) WHERE any(g IN t.files_owned WHERE g STARTS WITH 'src/') RETURN t");
    assert!(
        c.contains("(LISTPRED any 1 (PROP (VAR 0) \"files_owned\")"),
        "{c}"
    );
    // V1: each part of a composite query has its own scope.
    same_cast(
        "MATCH (a:task) RETURN a UNION MATCH (b:task) RETURN b",
        "MATCH (a:task) RETURN a UNION MATCH (a:task) RETURN a",
    );
}

#[test]
fn transactions_normalise_their_statements() {
    // N3: DELETE options in fixed slots.
    let a = write("TX { DELETE #40 REASON 'dup' REPLACED BY #52 RELEASE }");
    let b = write("TX { DELETE #40 RELEASE REPLACED BY #52 REASON 'dup' }");
    assert_eq!(encode(Root::Tx(&a.ast)), encode(Root::Tx(&b.ast)));
    // N5: EXPECT as (min, max).
    let s = sexpr(Root::Tx(
        &write("TX { MATCH (t:task) WHERE t.priority = 3 EXPECT >= 2 SET t.priority = 2 }").ast,
    ));
    assert!(s.contains("(EXPECT 2 _)"), "{s}");
    // N6 and §5.4: created edges in the stored direction; INSERT is CREATE.
    let a = write("TX { CREATE (#12)-[:BLOCKS]->(#51) }");
    let b = write("TX { INSERT (#51)-[:BLOCKED_BY]->(#12) }");
    assert_eq!(encode(Root::Tx(&a.ast)), encode(Root::Tx(&b.ast)));
    let s = sexpr(Root::Tx(
        &write("TX { CREATE (t:task {title: 'x'})-[:BLOCKED_BY]->(#12) UNDER #88 }").ast,
    ));
    assert!(s.contains("[[in \"BLOCKS\" [] (NODE"), "{s}");
    // ON, KEY, LEASE and DRY are outside the payload (§5.2).
    let a = write("TX ON main KEY 'k' LEASE 'L-1' { SET #12.priority = 1 } DRY");
    let b = write("TX { SET #12.priority = 'P1' }");
    assert_eq!(encode(Root::Tx(&a.ast)), encode(Root::Tx(&b.ast)));
    // A counter increment.
    write("TX { SET #12.reopen_count = #12.reopen_count + 1 }");
}

#[test]
fn the_std_lq_tx_instances_bind() {
    let p = Params::new()
        .with("title", Value::Text("t".into()))
        .with("if_rev", Value::Int(4466))
        .with("if_status", Value::Text("open".into()))
        .with("priority", Value::Int(1))
        .with("old", Value::Text("a".into()))
        .with("new", Value::Text("b".into()))
        .with("reason", Value::Text("r".into()))
        .with("failure_scenario", Value::Text("f".into()))
        .with("severity", Value::Text("important".into()))
        .with("f_kind", Value::Text("perf".into()))
        .with("text", Value::Text("body".into()))
        .with("summary", Value::Text("s".into()));
    for src in [
        "TX { CREATE (n:task {title: $title}) UNDER #88; CREATE (#12)-[:BLOCKS]->(n) }",
        "TX { MATCH (n {id: #12}) WHERE n.rev = $if_rev AND n.status = $if_status EXPECT 1 SET n.priority = $priority }",
        "TX { MATCH (#12)-[e:BLOCKS]->(#51) EXPECT 1 DELETE e }",
        "TX { PATCH #133.body REMOVE $old ADD $new; CREATE (#133)-[:DEPENDS_ON]->(#131) }",
        "TX ON lane/l10 KEY 'orch:rm-40' { DELETE #40 REPLACED BY #52 RELEASE REASON $reason } DRY",
        "TX { RESOLVE (CALL conflicts() YIELD key RETURN key) EXPECT >= 1 TAKE THEIRS }",
        "TX { CREATE (n:finding {title: $title, failure_scenario: $failure_scenario, severity: $severity, f_kind: $f_kind}); SET n.body = $text; CREATE (n)-[:ABOUT]->(#130) }",
        "TX LEASE 'L-18' { CALL tx.complete(#89, outcome: 'done', summary: $summary) YIELD ready }",
    ] {
        if let Err(e) = fixture::write_with(src, &p, &Caller::default()) {
            panic!("{src}: {}", fixture::show(src, &e));
        }
    }
}

// ----- the standard library -----------------------------------------------------------------------------------------

/// [LQ/std §9]: every definition parses, binds against the core schema, and classifies as its cursor column says.
#[test]
fn the_standard_library_binds_with_its_cursor_classes() {
    let live = [
        ("ready", true),
        ("blocking", true),
        ("blockers", true),
        ("tree", false),
        ("show", false),
        ("find", false),
        ("notes", false),
        ("changes", false),
        ("stale", true),
        ("conflicts", false),
        ("violations", false),
        ("history", false),
        ("blame", false),
        ("log", false),
        ("diff", false),
        ("across", false),
        ("loop", false),
        ("refuted_share", false),
        ("lane_conflicts", true),
        ("delta", false),
        ("links_broken", true),
        ("links_pending", true),
        ("links_proposals", true),
        ("links_guesses", false),
        ("files_removed", false),
        ("files_replaced", true),
        ("root_moves", false),
    ];
    let cat = crate::lq::catalog::std_catalog();
    assert_eq!(cat.len(), 40);
    for (name, is_live) in live {
        let q = crate::lq::catalog::std_query(name).unwrap_or_else(|| panic!("std.{name}"));
        assert_eq!(q.live, is_live, "std.{name}");
    }
    let show = crate::lq::catalog::std_query("show").unwrap();
    assert_eq!(
        show.params[0],
        ("ids".to_string(), crate::lq::catalog::PT::ListNode, true)
    );
    assert_eq!(show.columns[0].0, "n");
    // The portable text of a definition without store-local constants is the text itself.
    for t in crate::lq::catalog::STD_TEXTS {
        let b = fixture::define(t).unwrap_or_else(|e| panic!("{t}: {}", fixture::show(t, &e)));
        assert_eq!(b.portable[0].1, t);
    }
}

#[test]
fn named_queries_run_by_name() {
    let p = Params::new().with("scope", Value::Text("88".into()));
    let b = fixture::with_ctx(&p, &Caller::default(), |ctx| {
        crate::lq::bind::bind_named(ctx, "ready")
    })
    .unwrap();
    let s = sexpr(Root::Query(&b.ast));
    assert!(
        s.starts_with("(QUERY (PART _ (SCALL \"std.ready\" [(ARG \"scope\" (NODE"),
        "{s}"
    );
    assert!(b.live);
    same_cast("CALL ready(scope: 88)", "CALL std.ready(scope: #88)");
    let e = fixture::with_ctx(
        &Params::new().with("nope", Value::Int(1)),
        &Caller::default(),
        |ctx| crate::lq::bind::bind_named(ctx, "ready"),
    )
    .unwrap_err();
    assert_eq!((e[0].code, e[0].span), (Code::E110, None));
    let e = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_named(ctx, "show")
    })
    .unwrap_err();
    assert!(e[0].message.contains("needs"), "{}", e[0].message);
    let options =
        crate::lq::parser::parse_write("TX ON lane/x { SET #1.title = 'x' }", Default::default())
            .unwrap()
            .tree;
    let p = Params::new()
        .with("id", Value::Int(89))
        .with("outcome", Value::Text("done".into()))
        .with("summary", Value::Text("s".into()));
    let t = fixture::with_ctx(&p, &Caller::default(), |ctx| {
        crate::lq::bind::bind_mutation(ctx, "complete", &options)
    })
    .unwrap();
    assert!(sexpr(Root::Tx(&t.ast)).contains("(STXCALL \"tx.complete\""));
}

#[test]
fn project_named_queries_are_callable() {
    let mut schema = crate::lq::schema::Schema::core();
    schema.add_query("open_under", "DEFINE QUERY open_under($root: node) SHAPE node AS { MATCH (t:task) WHERE t IN subtree($root) AND t.status = 'open' RETURN t }");
    schema.add_kind("spike", &["open", "done"]);
    schema.add_field("spike", "budget_h", crate::lq::schema::FieldTy::Int);
    let ids = fixture::ids();
    let caller = Caller::default();
    let params = Params::new();
    let ctx = crate::lq::ctx::BindCtx {
        schema: &schema,
        ids: &ids,
        params: &params,
        caller: &caller,
    };
    let src = "CALL open_under(#88) YIELD t MATCH (s:spike) WHERE s.budget_h > 2 AND s.status = 'done' RETURN t, s";
    let p = crate::lq::parser::parse_read(src, Default::default()).unwrap();
    let b = crate::lq::bind::bind_read(&ctx, src, &p.tree)
        .unwrap_or_else(|e| panic!("{}", fixture::show(src, &e)));
    assert!(sexpr(Root::Query(&b.ast)).contains("(CALL \"open_under\" [(ARG \"root\" (NODE"));
}

// ----- binder errors ------------------------------------------------------------------------------------------------

#[test]
fn e001_unaliased_with_item() {
    assert_eq!(
        read_err("MATCH (t:task) WITH t.status RETURN t"),
        (Code::E001, 1, 21)
    );
}

#[test]
fn e101_unknown_field_with_suggestions_and_fixed_hint() {
    let src = "MATCH (t:task)\nWHERE t.stauts = 'open' RETURN t";
    assert_eq!(read_err(src), (Code::E101, 2, 9));
    let e = fixture::read_with(src, &Params::new(), &Caller::default()).unwrap_err();
    assert_eq!(e[0].message, "kind `task` has no field `stauts`");
    assert_eq!(e[0].inline.as_deref(), Some("did you mean `status`?"));
    assert!(
        e[0].help
            .as_deref()
            .unwrap()
            .starts_with("task fields: title, abstract, status, resolution, priority, ...")
    );
    let e = fixture::read_with(
        "MATCH (t:task) WHERE t.open RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(
        e[0].inline.as_deref(),
        Some("write t.status = 'open', or t.unfinished for any unfinished status")
    );
    assert_eq!(
        read_err("MATCH (n:note) WHERE n.done RETURN n").0,
        Code::E101
    );
    assert_eq!(
        read_err("MATCH (a)-[e:BLOCKS]->(b) RETURN e.anchor").0,
        Code::E101
    );
}

#[test]
fn e102_unknown_value_and_labels() {
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.status = 'opne' RETURN t"),
        (Code::E102, 1, 33)
    );
    let e = fixture::read_with(
        "MATCH (t:task) WHERE 'l5' IN labels(t) RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E102);
    assert_eq!(e[0].inline.as_deref(), Some("write 'l5' IN t.labels"));
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.priority = 7 RETURN t").0,
        Code::E102
    );
    read("MATCH (t) WHERE 'task' IN labels(t) RETURN t");
}

#[test]
fn e103_type_mismatches() {
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.estimate = 'x' RETURN t").0,
        Code::E103
    );
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.title < NULL RETURN t").0,
        Code::E103
    );
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.title AND t.done RETURN t").0,
        Code::E103
    );
    assert_eq!(read_err("MATCH (t:task) RETURN t.title - 1").0, Code::E103);
    assert_eq!(
        read_err("MATCH (s1:doc) WHERE s1.rev = s1 RETURN s1").0,
        Code::E103
    );
    assert_eq!(write_err("TX { SET #12.reopen_count = 3 }").0, Code::E103);
    assert_eq!(write_err("TX { SET #12.estimate = 'x' }").0, Code::E103);
}

#[test]
fn e104_e105_e107_names_in_patterns() {
    let e = fixture::read_with(
        "MATCH (a)-[:BLOCKZ]->(b) RETURN a",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].inline.as_deref()),
        (Code::E104, Some("did you mean `BLOCKS`?"))
    );
    assert_eq!(read_err("MATCH (a:tsk) RETURN a"), (Code::E105, 1, 10));
    assert_eq!(
        read_err("MATCH (a)-[:parent]->(b) RETURN a"),
        (Code::E107, 1, 13)
    );
    assert_eq!(read_err("MATCH (a)-[:PARENT]->(b) RETURN a").0, Code::E107);
    assert_eq!(read_err("MATCH (t) WHERE t:tsk RETURN t").0, Code::E105);
}

#[test]
fn e106_edge_direction() {
    let e = fixture::read_with(
        "MATCH (t:task)-[:GATES]->(v:verdict) RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E106);
    assert_eq!(
        e[0].inline.as_deref(),
        Some("write (t:task)<-[:GATES]-(v:verdict)")
    );
    let e = fixture::read_with(
        "MATCH (a:task)-[:DEPENDS_ON]->(b:task) RETURN a",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert!(
        e[0].message
            .starts_with("DEPENDS_ON links doc sections (doc -> doc)"),
        "{}",
        e[0].message
    );
    assert_eq!(
        read_err("MATCH (#12)-[:ANSWERS]->(q) RETURN q").0,
        Code::E106
    );
    assert_eq!(
        write_err("TX { CREATE (#12)-[:GATES]->(#51) }").0,
        Code::E106
    );
    // Symmetric kinds and any → any kinds never exclude a direction.
    read("MATCH (r:rule)-[:CONTRADICTS]-(#212) RETURN r");
    read("MATCH (#212)<-[:CONTRADICTS]-(r:rule) RETURN r");
}

#[test]
fn e108_bare_words() {
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.status = opne RETURN t"),
        (Code::E108, 1, 33)
    );
    assert_eq!(read_err("MATCH (t:task) RETURN x").0, Code::E108);
}

#[test]
fn e109_functions_and_procedures() {
    assert_eq!(
        read_err("MATCH (t:task) RETURN lowr(t.title)").0,
        Code::E109
    );
    assert_eq!(
        read_err("MATCH (t:task) RETURN lower(t.title, 2)").0,
        Code::E109
    );
    assert_eq!(
        read_err("MATCH (t:task) RETURN subtree(t, deep: 2)").0,
        Code::E109
    );
    assert_eq!(read_err("MATCH (t:task) RETURN diff(t)").0, Code::E109);
    assert_eq!(read_err("CALL complete(#1)").0, Code::E109);
    assert_eq!(read_err("CALL nosuch()").0, Code::E109);
    assert_eq!(
        read_err("CALL blockers(#51) YIELD blockr RETURN blockr").0,
        Code::E109
    );
    assert_eq!(read_err("CALL blockers(#51, depth: 2)").0, Code::E109);
    assert_eq!(read_err("CALL blockers()").0, Code::E109);
    assert_eq!(write_err("TX { CALL tx.completes(#89) }").0, Code::E109);
}

#[test]
fn e110_parameters() {
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.id = $x RETURN t"),
        (Code::E110, 1, 29)
    );
    let p = Params::new().with("x", Value::Text("abc".into()));
    assert_eq!(
        read_err_with(
            "MATCH (t:task) WHERE t.id = $x RETURN t",
            &p,
            &Caller::default()
        )
        .0,
        Code::E110
    );
    let p = Params::new().with("s", Value::Text("nope".into()));
    assert_eq!(
        read_err_with(
            "MATCH (t:task) WHERE t.status = $s RETURN t",
            &p,
            &Caller::default()
        )
        .0,
        Code::E110
    );
    let p = Params::new().with("f", Value::Float(f64::NAN));
    assert_eq!(
        read_err_with(
            "MATCH (m:measurement) WHERE m.value = $f RETURN m",
            &p,
            &Caller::default()
        )
        .0,
        Code::E110
    );
    assert_eq!(read_err("CALL ready(scop: #88)").0, Code::E110);
    assert_eq!(read_err("CALL show()").0, Code::E110);
    let e =
        fixture::define("DEFINE QUERY q($x: widget) AS { MATCH (t:task) RETURN t }").unwrap_err();
    assert_eq!(e[0].code, Code::E110);
    let e = fixture::define(
        "DEFINE QUERY q($x: int) AS { MATCH (t:task) WHERE t.estimate = $y RETURN t }",
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E110);
    let e = fixture::define("DEFINE QUERY q($x: int = 'a') AS { MATCH (t:task) RETURN t }")
        .unwrap_err();
    assert_eq!(e[0].code, Code::E110);
}

#[test]
fn e111_ids_never_allocated() {
    let e = fixture::read_with(
        "MATCH (t {id: #9999}) RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].message.as_str(), e[0].help.as_deref()),
        (
            Code::E111,
            "#9999 was never allocated in this store",
            Some("next id is #3001")
        )
    );
    assert_eq!(
        read_err("MATCH (t) WHERE t = #u:0123456789abcdef0123456789abcdef RETURN t").0,
        Code::E111
    );
    assert_eq!(
        read_err("MATCH (t) WHERE t.id = 4000 RETURN t").0,
        Code::E111
    );
}

#[test]
fn e112_aggregates() {
    let e = fixture::read_with(
        "MATCH (t:task) WHERE count(*) > 1 RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].message.as_str()),
        (Code::E112, "aggregate count() inside WHERE")
    );
    assert_eq!(
        read_err("MATCH (t:task) RETURN sum(count(*))").0,
        Code::E112
    );
    assert_eq!(
        read_err("MATCH (t:task) RETURN t.status, count(*) GROUP BY t.priority").0,
        Code::E112
    );
    assert_eq!(
        read_err("MATCH (t:task) UNWIND collect(t) AS x RETURN x").0,
        Code::E112
    );
    read("MATCH (b:task)-[:BLOCKS]->() WITH b, count(*) AS n WHERE n > 1 RETURN b, n");
}

#[test]
fn e113_edge_variable_on_a_quantified_edge() {
    let e = fixture::read_with(
        "MATCH (a)-[e:BLOCKS*1..3]->(b) RETURN b",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E113);
    assert_eq!(
        e[0].inline.as_deref(),
        Some("write a quantified group: (x)((a)-[e:BLOCKS]->(b) WHERE ...){1,3}(y)")
    );
    assert_eq!(
        read_err("MATCH (a)-[e:BLOCKS]->+(b) RETURN b").0,
        Code::E113
    );
}

#[test]
fn e115_writes_the_schema_refuses() {
    assert_eq!(
        write_err("TX { SET #12.ready = true }"),
        (Code::E115, 1, 14)
    );
    assert_eq!(write_err("TX { SET #12.unblocked = true }").0, Code::E115);
    assert_eq!(write_err("TX { SET #303.path = 'a/b' }").0, Code::E115);
    assert_eq!(
        write_err("TX { SET #303.origin_path = 'a/b' }").0,
        Code::E115
    );
    assert_eq!(write_err("TX { SET #300.root = 'x' }").0, Code::E115);
    assert_eq!(
        write_err("TX { SET #303.status = 'removed' }").0,
        Code::E115
    );
    assert_eq!(write_err("TX { SET #12.id = #13 }").0, Code::E115);
    assert_eq!(
        write_err("TX { CREATE (f:artifact {title: 'x'}) }").0,
        Code::E115
    );
    assert_eq!(write_err("TX { CREATE (#12)-[:AT]->(#303) }").0, Code::E115);
    assert_eq!(
        write_err("TX { MATCH (a)-[e:BLOCKS]->(b) EXPECT 1 SET e.flagged = true }").0,
        Code::E115
    );
    assert_eq!(
        write_err("TX { CALL tx.link_file(#12, spec: 'a.rs') }").0,
        Code::E115
    );
    write("TX { SET #12.done = true }");
}

#[test]
fn e116_step_variables_stay_in_their_group() {
    let src = "MATCH (x)((a)-[:BLOCKS]->(b))+(y) RETURN a";
    assert_eq!(read_err(src), (Code::E116, 1, 42));
    read("MATCH (x)((a)-[:BLOCKS]->(b) WHERE b.unfinished)+(y) RETURN x, y");
}

#[test]
fn e117_store_local_constants_in_a_definition() {
    let e =
        fixture::define("DEFINE QUERY q() AS { USE main@3 MATCH (t:task) RETURN t }").unwrap_err();
    assert_eq!(e[0].code, Code::E117);
    let e = fixture::define("DEFINE QUERY q() AS { USE main@2026-01-02 MATCH (t:task) RETURN t }")
        .unwrap_err();
    assert_eq!(e[0].code, Code::E117);
    let e = fixture::define(
        "DEFINE QUERY q() AS { MATCH (n)-[a:AT]->(f) WHERE a.anchor = 'a17' RETURN n }",
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E117);
    // Outside a definition both are allowed.
    read("USE main@3 MATCH (t:task) RETURN t");
    read("MATCH (n)-[a:AT]->(f) WHERE a.anchor = 'a17' RETURN n");
}

#[test]
fn e301_unknown_and_ambiguous_revisions() {
    assert_eq!(
        read_err("USE s9999 MATCH (t:task) RETURN t"),
        (Code::E301, 1, 5)
    );
    assert_eq!(
        read_err("USE cdeadbee MATCH (t:task) RETURN t").0,
        Code::E301
    );
    let e = fixture::read_with(
        "USE cc000000 MATCH (t:task) RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E301);
    assert!(
        e[0].message.ends_with("matches 5000 commits") || e[0].message.contains("matches"),
        "{}",
        e[0].message
    );
    assert!(e[0].detail.len() <= 5);
    assert_eq!(
        read_err("MATCH (t:task) WHERE t.rev = 99999 RETURN t").0,
        Code::E301
    );
}

#[test]
fn e302_runtime_and_tree_state_at_a_past_view() {
    let e = fixture::read_with(
        "USE main~3 MATCH (t:task) WHERE t.ready RETURN t",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].inline.as_deref()),
        (
            Code::E302,
            Some("use t.unblocked (structural, valid at any version)")
        )
    );
    assert_eq!(
        read_err("USE main~20 MATCH (f {id: #307}) RETURN link_state(f)").0,
        Code::E302
    );
    assert_eq!(read_err("USE tags/m1 CALL leases()").0, Code::E302);
    let caller = Caller {
        tree: false,
        ..Caller::default()
    };
    let e = fixture::read_with(
        "MATCH (n)-[a:AT]->(f) RETURN link_state(a)",
        &Params::new(),
        &caller,
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].help.as_deref()),
        (Code::E302, Some("pass --tree DIR (the query tool: tree)"))
    );
    let caller = Caller {
        at: Some("main~2".into()),
        ..Caller::default()
    };
    assert_eq!(
        read_err_with(
            "MATCH (t:task) WHERE t.claimed RETURN t",
            &Params::new(),
            &caller
        )
        .0,
        Code::E302
    );
    read("USE main~3 MATCH (t:task) WHERE t.unblocked RETURN t");
    read("USE lane/x MATCH (t:task) WHERE t.ready RETURN t");
}

#[test]
fn e304_too_many_views() {
    assert_eq!(read_err("CALL across(refs: [main, lane/a, lane/b, lane/c, lane/d], ids: [#12]) YIELD node RETURN node").0, Code::E304);
    let src = "USE lane/a MATCH (t:task) RETURN t UNION USE lane/b MATCH (t:task) RETURN t UNION USE lane/c MATCH (t:task) RETURN t UNION USE lane/d MATCH (t:task) RETURN t UNION MATCH (t:task) RETURN t";
    assert_eq!(read_err(src).0, Code::E304);
    read("CALL across(refs: [main, lane/a], ids: [#12]) YIELD node RETURN node");
}

#[test]
fn e305_read_only_views() {
    assert_eq!(
        write_err("TX ON c41d7e0b { SET #12.title = 'x' }"),
        (Code::E305, 1, 7)
    );
    assert_eq!(
        write_err("TX ON main~2 { SET #12.title = 'x' }").0,
        Code::E305
    );
    assert_eq!(
        write_err("TX ON tags/m1 { SET #12.title = 'x' }").0,
        Code::E305
    );
    assert_eq!(
        write_err("TX ON merge/main/from/lane/x { SET #12.title = 'x' }").0,
        Code::E305
    );
    assert_eq!(
        write_err("TX ON plan/q3 { SET #12.status = 'done' }").0,
        Code::E305
    );
    assert_eq!(
        write_err("TX ON plan/q3 { CALL tx.claim(ids: [#12]) }").0,
        Code::E305
    );
    write("TX ON plan/q3 { SET #12.priority = 1 }");
    write("TX ON merge/main/from/lane/x { RESOLVE 'k' TAKE OURS }");
}

#[test]
fn e406_role_policy_of_the_binder() {
    let mcp = Caller {
        surface: Surface::Mcp,
        ..Caller::default()
    };
    let e = fixture::write_with("TX { DELETE #40 }", &Params::new(), &mcp).unwrap_err();
    assert_eq!((e[0].code, e[0].span), (Code::E406, None));
    assert_eq!(
        e[0].message,
        "`node DELETE` runs only through moirai tx, for the orchestrator and the owner"
    );
    assert_eq!(
        write_err_with("TX { RESOLVE 'k' TAKE OURS }", &Params::new(), &mcp).0,
        Code::E406
    );
    assert_eq!(
        write_err_with(
            "TX { DEFINE QUERY q() AS { MATCH (t:task) RETURN t } }",
            &Params::new(),
            &mcp
        )
        .0,
        Code::E406
    );
    let dev = Caller {
        role: "developer".into(),
        ..Caller::default()
    };
    let e = fixture::write_with(
        "TX { MATCH (t:task) WHERE t.priority = 3 EXPECT >= 1 SET t.priority = 2 }",
        &Params::new(),
        &dev,
    )
    .unwrap_err();
    assert_eq!(
        (e[0].code, e[0].message.as_str()),
        (
            Code::E406,
            "statement 1: role `developer` may not write a bulk target"
        )
    );
    assert_eq!(
        e[0].detail.to_vec(),
        vec!["nothing was written".to_string()]
    );
    assert_eq!(
        write_err_with("TX { REOPEN #12 REASON 'r' }", &Params::new(), &dev).0,
        Code::E406
    );
    assert_eq!(
        write_err_with("TX { DELETE #40 }", &Params::new(), &dev).0,
        Code::E406
    );
    fixture::write_with(
        "TX { CALL tx.complete(#89, outcome: 'done', summary: 's') }",
        &Params::new(),
        &dev,
    )
    .unwrap();
    let gp = Caller {
        role: "general-purpose".into(),
        ..Caller::default()
    };
    assert_eq!(
        write_err_with(
            "TX { CALL tx.complete(#89, outcome: 'done', summary: 's') }",
            &Params::new(),
            &gp
        )
        .0,
        Code::E406
    );
    let safe = Caller {
        named_only: true,
        ..Caller::default()
    };
    assert_eq!(
        read_err_with("MATCH (t:task) RETURN t", &Params::new(), &safe),
        (Code::E406, 0, 0)
    );
    fixture::with_ctx(&Params::new(), &safe, |ctx| {
        crate::lq::bind::bind_named(ctx, "blocking")
    })
    .unwrap();
}

#[test]
fn e411_unknown_profile() {
    let unknown = Caller {
        profile: Profile::Unknown,
        ..Caller::default()
    };
    let e =
        fixture::write_with("TX { SET #12.priority = 1 }", &Params::new(), &unknown).unwrap_err();
    assert_eq!((e[0].code, e[0].span), (Code::E411, None));
    assert_eq!(
        e[0].detail.to_vec(),
        vec!["use the named mutation tx.set (the write tool: name and params)".to_string()]
    );
    let e = fixture::write_with(
        "TX { SET #12.priority = 1; MOVE #12 UNDER #88 }",
        &Params::new(),
        &unknown,
    )
    .unwrap_err();
    assert_eq!(
        e[0].detail.to_vec(),
        vec!["no named mutation matches; ask the orchestrator".to_string()]
    );
    let dry = Caller {
        unknown_dry_targets: true,
        ..unknown.clone()
    };
    fixture::write_with("TX { SET #12.priority = 1 } DRY", &Params::new(), &dry).unwrap();
    assert_eq!(
        write_err_with("TX { SET #12.priority = 1 }", &Params::new(), &dry).0,
        Code::E411
    );
    let options = crate::lq::parser::parse_write("TX { SET #1.title = 'x' }", Default::default())
        .unwrap()
        .tree;
    let p = Params::new()
        .with("id", Value::Int(12))
        .with("reason", Value::Text("r".into()));
    fixture::with_ctx(&p, &unknown, |ctx| {
        crate::lq::bind::bind_mutation(ctx, "reopen", &options)
    })
    .unwrap();
}

#[test]
fn at_most_three_errors_in_source_order() {
    let e = fixture::read_with(
        "MATCH (a:x1)-[:Y1]->(b:x2) WHERE a.q = 1 RETURN zz, yy",
        &Params::new(),
        &Caller::default(),
    )
    .unwrap_err();
    assert_eq!(e.len(), 3);
    assert!(e.windows(2).all(|w| w[0].start() <= w[1].start()));
}

// ----- warnings and notices -----------------------------------------------------------------------------------------

fn lints(src: &str) -> Vec<(Code, LintKind)> {
    read(src)
        .lints
        .into_iter()
        .map(|l| (l.code, l.kind))
        .collect()
}

#[test]
fn w01_absent_decided_comparisons() {
    let l = lints("MATCH (t:task) WHERE t.estimate > 2 RETURN t");
    assert_eq!(
        l,
        vec![(
            Code::W01,
            LintKind::W01 {
                field: "estimate".into(),
                expr: "t.estimate".into()
            }
        )]
    );
    let b = read("MATCH (t:task) WHERE t.estimate > 2 RETURN t");
    assert_eq!(
        b.lints[0].message(Some(12)),
        "12 rows excluded because estimate is absent; use coalesce(t.estimate, 0) or t.estimate IS NULL"
    );
    assert!(lints("MATCH (t:task) WHERE t.priority <= 1 RETURN t").is_empty());
    assert!(lints("MATCH (t:task) WHERE t.estimate = 2 RETURN t").is_empty());
}

#[test]
fn w02_match_modes() {
    assert_eq!(
        lints("MATCH TRAIL (t:task) RETURN t"),
        vec![(
            Code::W02,
            LintKind::W02 {
                mode: "TRAIL".into()
            }
        )]
    );
    assert!(lints("MATCH DIFFERENT EDGES (t:task) RETURN t").is_empty());
}

#[test]
fn w07_hand_derived_readiness() {
    let w07 = |v: &str| (Code::W07, LintKind::W07 { var: v.into() });
    assert_eq!(
        lints("MATCH (t:task) WHERE t.status = 'open' AND NOT (t)<-[:BLOCKS]-() RETURN t"),
        vec![w07("t")]
    );
    assert_eq!(
        lints(
            "MATCH (t:task) WHERE NOT EXISTS { MATCH (b)-[:BLOCKS]->(t) WHERE b.status <> 'done' } RETURN t"
        ),
        vec![w07("t")]
    );
    assert_eq!(
        lints("MATCH (t:task) WHERE COUNT { (b)-[:BLOCKS]->(t) } = 0 RETURN t"),
        vec![w07("t")]
    );
    assert_eq!(
        lints("MATCH (b)-[:BLOCKS]->(t:task) WHERE b.status = 'done' RETURN t"),
        vec![w07("t")]
    );
    assert!(
        lints("MATCH (t:task) WHERE NOT (t)<-[:BLOCKS]-() AND t.unblocked RETURN t").is_empty()
    );
    assert!(lints("MATCH (t:task) WHERE EXISTS { (b)-[:BLOCKS]->(t) } RETURN t").is_empty());
}

#[test]
fn w10_link_state_inequality() {
    assert_eq!(
        lints("MATCH (t:task) WHERE link_state(t) <> 'ok' RETURN t"),
        vec![(
            Code::W10,
            LintKind::W10 {
                var: "t".into(),
                op: "<>".into()
            }
        )]
    );
    assert_eq!(
        lints("MATCH (t:task) WHERE NOT link_state(t) IN ['ok'] RETURN t"),
        vec![(
            Code::W10,
            LintKind::W10 {
                var: "t".into(),
                op: "NOT IN".into()
            }
        )]
    );
    assert!(lints("MATCH (n)-[a:AT]->(f) WHERE link_state(a) <> 'ok' RETURN f").is_empty());
    assert!(lints("MATCH (f:artifact) WHERE link_state(f) <> 'ok' RETURN f").is_empty());
}

#[test]
fn n08_aggregates_over_quantified_parts() {
    let b = read("MATCH (x:task)-[:BLOCKS]->{2,}(#93) RETURN count(*)");
    assert_eq!(b.lints.len(), 1);
    assert_eq!(b.lints[0].code, Code::N08);
    assert_eq!(
        b.lints[0].message(None),
        "count(*) over a quantified pattern counts (x, #93) endpoint pairs, not paths"
    );
    assert!(lints("MATCH (x:task)-[:BLOCKS]->(#93) RETURN count(*)").is_empty());
    assert!(
        lints("MATCH (x:task) WHERE EXISTS { (x)-[:BLOCKS]->+(#93) } RETURN count(*)").is_empty()
    );
}

// ----- the reading echo ---------------------------------------------------------------------------------------------

fn reads_as(src: &str, caller: &Caller) -> Vec<String> {
    fixture::read_with(src, &Params::new(), caller)
        .unwrap_or_else(|e| panic!("{}", fixture::show(src, &e)))
        .reads
}

#[test]
fn reading_echo_of_envelope_4() {
    let gated = Caller {
        profile: Profile::Gated,
        ..Caller::default()
    };
    let compat = Caller::default();
    // [50 §2.9] Q4: a reverse alias prints the stored reading and the written form.
    assert_eq!(
        reads_as("MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b", &gated),
        vec!["b BLOCKS #51 (written #51 BLOCKED_BY b) | b must finish before #51 starts"]
    );
    // Mistakes 4 and 5 of [LQ/errors §7].
    assert_eq!(
        reads_as("MATCH (#51)-[:BLOCKS]->(b) RETURN b", &gated),
        vec!["#51 BLOCKS b | #51 must finish before b starts"]
    );
    assert_eq!(
        reads_as("MATCH (p {id: #88})-[:CHILD_OF]->(c:task) RETURN c", &gated),
        vec!["#88 CHILD_OF c | #88 is a child of c"]
    );
    // Gated: unanchored or not same-kind hops are silent; compatible echoes every edge pattern.
    assert!(reads_as("MATCH (a)-[:BLOCKS]->(b) RETURN b", &gated).is_empty());
    assert!(reads_as("MATCH (v)-[:GATES]->(#51) RETURN v", &gated).is_empty());
    assert_eq!(
        reads_as("MATCH (v)-[:GATES]->(#51) RETURN v", &compat),
        vec!["v GATES #51 | verdict v gates the completion of #51"]
    );
    // Quantifiers in both display spellings, with the reading's suffix; a group of one edge; undirected patterns.
    let gql = Caller {
        display: Spelling::Gql,
        ..Caller::default()
    };
    assert_eq!(
        reads_as("MATCH (x:task)-[:BLOCKS*1..5]->(#93) RETURN x", &gql),
        vec!["x BLOCKS{1,5} #93 | x must finish before #93 starts (through 1 to 5 steps)"]
    );
    assert_eq!(
        reads_as("MATCH (x:task)-[:BLOCKS]->+(#93) RETURN x", &compat),
        vec!["x BLOCKS*1.. #93 | x must finish before #93 starts (through 1 or more steps)"]
    );
    assert_eq!(
        reads_as("MATCH (x)((a)-[:BLOCKS]->(b)){2}(#93) RETURN x", &gql),
        vec![
            "a BLOCKS b | a must finish before b starts",
            "x BLOCKS{2} #93 | x must finish before #93 starts (through exactly 2 steps)"
        ]
    );
    assert_eq!(
        reads_as(
            "MATCH (x)((a)-[:BLOCKS]->(b)-[:CHILD_OF]->(c)){1,2}(y) RETURN x",
            &gql
        )[2],
        "x (BLOCKS CHILD_OF){1,2} y | x reaches y through BLOCKS then CHILD_OF (through 1 to 2 steps)"
    );
    assert_eq!(
        reads_as("MATCH (a)-[:BLOCKS]-(#51) RETURN a", &compat),
        vec![
            "a BLOCKS #51 (either direction) | a must finish before #51 starts, or #51 must finish before a starts"
        ]
    );
    // Each distinct line once; patterns inside EXISTS echo too.
    assert_eq!(reads_as("MATCH (t:task) WHERE EXISTS { (t)<-[:BLOCKS]-(b) } AND EXISTS { (t)<-[:BLOCKS]-(b) } RETURN t", &compat).len(), 1);
}

// ----- definitions and their portable form --------------------------------------------------------------------------

#[test]
fn definitions_keep_parameters_and_rewrite_store_local_constants() {
    let src = "DEFINE QUERY near_88($d: int = 2) SHAPE Node BUDGET Light AS {\r\n  USE s4466  \r\n  MATCH (t {id: 51})-[:CHILD_OF]->(#88)\n  WHERE t.rev = 4466 AND t.estimate <= $d\n  RETURN t\n}";
    let b = fixture::define(src).unwrap_or_else(|e| panic!("{}", fixture::show(src, &e)));
    let s = sexpr(Root::Define(&b.ast));
    assert!(
        s.starts_with(
            "(DEFINE \"near_88\" [(PDECL \"d\" (TYPE \"int\" _) false (INT 2))] \"node\" \"light\""
        ),
        "{s}"
    );
    assert!(s.contains("(PARAM 0)"), "{s}");
    let u51 = super::fixture::uid(51)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let u88 = super::fixture::uid(88)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let c = super::fixture::commit(4466)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let want = format!(
        "DEFINE QUERY near_88($d: int = 2) SHAPE Node BUDGET Light AS {{\n  USE c{c}\n  MATCH (t {{id: #u:{u51}}})-[:CHILD_OF]->(#u:{u88})\n  WHERE t.rev = 'c{c}' AND t.estimate <= $d\n  RETURN t\n}}"
    );
    assert_eq!(b.portable, vec![("near_88".to_string(), want.clone())]);
    // The portable text binds to the same C-AST (the F3 hash is the same in every store, [50 §8.3]).
    let again = fixture::define(&want).unwrap();
    assert_eq!(
        encode(Root::Define(&again.ast)),
        encode(Root::Define(&b.ast))
    );
    assert_eq!(again.portable[0].1, want);
    // Inside a TX the definition is a statement; its portable text is reported.
    let t = write(
        "TX { DEFINE QUERY q($id: node) AS { MATCH (t) WHERE t = $id OR t = #12 RETURN t } }",
    );
    assert!(t.portable[0].1.contains("#u:"), "{:?}", t.portable);
}

// ----- properties ---------------------------------------------------------------------------------------------------

/// Binds `text` read back from a printed tree: `Ok(encoding)` or `Err(first code)`.
/// What a bind decided: the encoding, the lints, the echo and the cursor class; or the first error code.
type Summary = Result<(Vec<u8>, Vec<crate::lq::bind::Lint>, Vec<String>, bool), Code>;

fn summary<T>(
    b: Result<crate::lq::bind::Bound<T>, Vec<crate::lq::diag::Diag>>,
    enc: impl Fn(&T) -> Vec<u8>,
) -> Summary {
    b.map(|b| (enc(&b.ast), b.lints, b.reads, b.live))
        .map_err(|e| e[0].code)
}

fn bound_read(text: &str) -> Summary {
    let p = crate::lq::parser::parse_read(text, Default::default()).map_err(|e| e[0].code)?;
    let b = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_read(ctx, text, &p.tree)
    });
    summary(b, |a| encode(Root::Query(a)))
}

fn bound_tx(text: &str) -> Summary {
    let p = crate::lq::parser::parse_write(text, Default::default()).map_err(|e| e[0].code)?;
    let b = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_write(ctx, text, &p.tree)
    });
    summary(b, |a| encode(Root::Tx(a)))
}

fn bound_define(text: &str) -> Summary {
    let p = crate::lq::parser::parse_define(text, Default::default()).map_err(|e| e[0].code)?;
    let b = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_define(ctx, text, &p.tree)
    });
    summary(b, |a| encode(Root::Define(a)))
}

/// [LQ/canonical-ast §1.3]: the canonical form, its encoding and every hash are independent of the display spelling,
/// and binding is deterministic: a tree printed in either spelling binds alike (the same bytes, or the same first code).
#[test]
fn the_canonical_form_ignores_the_display_spelling() {
    use crate::lq::printer::{print_define, print_read, print_tx};
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;
    crate::lq::parser::on_front_end_stack(|| {
        let mut runner = super::runner(300);
        let check_read = |r: crate::lq::ast::Read| -> Result<(), TestCaseError> {
            let a = print_read(&r, Spelling::Cypher);
            let b = print_read(&r, Spelling::Gql);
            prop_assert_eq!(bound_read(&a), bound_read(&b), "\n{}\n{}", a, b);
            prop_assert_eq!(bound_read(&a), bound_read(&a));
            Ok(())
        };
        if let Err(e) = runner.run(&super::strat::read(), check_read) {
            panic!("{e}");
        }
        let check_tx = |t: crate::lq::ast::Tx| -> Result<(), TestCaseError> {
            let a = print_tx(&t, Spelling::Cypher);
            let b = print_tx(&t, Spelling::Gql);
            prop_assert_eq!(bound_tx(&a), bound_tx(&b), "\n{}\n{}", a, b);
            Ok(())
        };
        if let Err(e) = runner.run(&super::strat::tx(), check_tx) {
            panic!("{e}");
        }
        let check_define = |d: crate::lq::ast::Define| -> Result<(), TestCaseError> {
            let a = print_define(&d, Spelling::Cypher);
            let b = print_define(&d, Spelling::Gql);
            prop_assert_eq!(
                bound_define(&a),
                bound_define(&b),
                "
{}
{}",
                a,
                b
            );
            Ok(())
        };
        if let Err(e) = runner.run(&super::strat::define(), check_define) {
            panic!("{e}");
        }
    });
}

#[test]
fn a_bound_use_parameter_selects_the_view() {
    let p = Params::new().with("r", Value::Text("main~2".into()));
    assert_eq!(
        read_err_with(
            "USE $r MATCH (t:task) WHERE t.ready RETURN t",
            &p,
            &Caller::default()
        )
        .0,
        Code::E302
    );
    let p = Params::new().with("r", Value::Text("lane/x".into()));
    fixture::read_with(
        "USE $r MATCH (t:task) WHERE t.ready RETURN t",
        &p,
        &Caller::default(),
    )
    .unwrap();
}

// ----- robustness ---------------------------------------------------------------------------------------------------

#[test]
fn long_operator_chains_bind_and_encode() {
    let mut src = String::from("MATCH (t:task) RETURN t.estimate");
    for _ in 0..20_000 {
        src.push_str(" + 1");
    }
    let b = read(&src);
    assert!(encode(Root::Query(&b.ast)).len() > 20_000 * 10);
}

#[test]
fn subqueries_restore_the_scope_and_kinds_around_them() {
    // A WITH inside a clause subquery replaces the subquery's scope only.
    read(
        "MATCH (t:task) WHERE EXISTS { MATCH (t)-[:BLOCKS]->(x) WITH x WHERE x.unfinished RETURN x } RETURN t",
    );
    // An existence test does not narrow an outer variable's kinds ([LQ/std] find).
    read("MATCH (n) WHERE EXISTS { (n)-[:SCOPED_TO]->(a) } AND n.done RETURN n");
    read("MATCH (n) OPTIONAL MATCH (n)-[:SCOPED_TO]->(a) RETURN n.done, a");
    // A step variable of a group inside a subquery is not a step variable outside it.
    assert_eq!(
        read_err("MATCH (t) WHERE EXISTS { (t)((a)-[:BLOCKS]->(b))+(y) } RETURN a").0,
        Code::E108
    );
}

#[test]
fn durations_scale_by_integers() {
    // [50 §2.9] Q21.
    let src = "DEFINE QUERY stale_blockers($scope: node = #9, $days: int = 3) SHAPE node AS {\n  MATCH (b:task)-[:BLOCKS]->(t:task)\n  WHERE t IN subtree($scope)\n    AND b.status = 'in_progress' AND b.updated_at < now() - $days * 1d\n  RETURN DISTINCT b ORDER BY b.updated_at\n}";
    let b = fixture::define(src).unwrap_or_else(|e| panic!("{}", fixture::show(src, &e)));
    assert!(b.portable[0].1.contains("$scope: node = #u:"));
    assert_eq!(
        read_err("MATCH (t:task) RETURN t.updated_at * 1d").0,
        Code::E103
    );
}

#[test]
fn transaction_matches_echo_their_patterns() {
    let t = write("TX { MATCH (#51)-[e:BLOCKED_BY]->(b) EXPECT 1 DELETE e }");
    assert_eq!(
        t.reads,
        vec![
            "b BLOCKS #51 (written #51 BLOCKED_BY b) | b must finish before #51 starts".to_string()
        ]
    );
}

// ----- review of WP-93a ---------------------------------------------------------------------------------------------

/// V5: a `RETURN`'s `ORDER BY` resolves an alias of the same `RETURN` anywhere in a sort key, not only as the whole
/// key; a variable bound inside the key (a list predicate's) shadows it.
#[test]
fn order_by_aliases_resolve_anywhere_in_a_key() {
    let s = cast_of("MATCH (t:task) RETURN t.title AS p ORDER BY p + 'x'");
    assert!(
        s.contains("[(SORT (ARITH + (ITEMREF 0) (TEXT \"x\")) false)]"),
        "{s}"
    );
    let s = cast_of("RETURN 1 AS x ORDER BY -x");
    assert!(s.contains("[(SORT (NEG (ITEMREF 0)) false)]"), "{s}");
    let s = cast_of("MATCH (t:task) RETURN t AS n, 2 AS k ORDER BY n.priority, k DESC");
    assert!(
        s.contains("[(SORT (PROP (ITEMREF 0) \"priority\") false) (SORT (ITEMREF 1) true)]"),
        "{s}"
    );
    let s = cast_of("RETURN [1] AS x ORDER BY all(x IN [2] WHERE x > 1)");
    assert!(
        s.contains("(LISTPRED all 0 (LIST [(INT 2)]) (CMP > (VAR 0) (INT 1)))"),
        "{s}"
    );
    // The alias takes the item's type: a node's property checks against its kinds.
    assert_eq!(
        read_err("MATCH (t:task) RETURN t AS n ORDER BY n.stauts").0,
        Code::E101
    );
    // Only the RETURN's own aliases: WITH items are bindings of their own (V4).
    assert_eq!(
        read_err("MATCH (t:task) WITH t AS n RETURN n AS m ORDER BY zz").0,
        Code::E108
    );
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// [LQ/canonical-ast §8.1] step 4: a quoted revision range or list rewrites every store-local base, both ends of a
/// range and every list element, suffixes kept, in one replacement literal.
#[test]
fn portable_form_of_quoted_ranges_and_lists() {
    let c = |s: u64| hex_of(&super::fixture::commit(s));
    for (written, portable) in [
        ("'s12..main'", format!("'c{}..main'", c(12))),
        ("'main..s12'", format!("'main..c{}'", c(12))),
        ("'[s1, main]'", format!("'[c{}, main]'", c(1))),
        ("'s12~1...s13^2'", format!("'c{}~1...c{}^2'", c(12), c(13))),
        ("\"s7\"", format!("'c{}'", c(7))),
        ("'main..lane/x'", "'main..lane/x'".to_string()),
    ] {
        let src =
            format!("DEFINE QUERY q() AS {{ CALL log({written}) YIELD commit RETURN commit }}");
        let b = fixture::define(&src).unwrap_or_else(|e| panic!("{}", fixture::show(&src, &e)));
        let want =
            format!("DEFINE QUERY q() AS {{ CALL log({portable}) YIELD commit RETURN commit }}");
        assert_eq!(b.portable[0].1, want, "{written}");
        // The portable text binds to the same C-AST.
        let again =
            fixture::define(&want).unwrap_or_else(|e| panic!("{}", fixture::show(&want, &e)));
        assert_eq!(
            encode(Root::Define(&again.ast)),
            encode(Root::Define(&b.ast)),
            "{written}"
        );
    }
}

/// [LQ/canonical-ast §5.6]: a commit literal no commit starts with is E301, a full 64-digit id included.
#[test]
fn e301_full_commit_ids_the_store_does_not_know() {
    let unknown = "ab".repeat(32);
    assert_eq!(
        read_err(&format!("USE c{unknown} MATCH (t:task) RETURN t")),
        (Code::E301, 1, 5)
    );
    let known = hex_of(&super::fixture::commit(12));
    read(&format!("USE c{known} MATCH (t:task) RETURN t"));
}

/// A schema with project named queries, and a read bound against it: its C-AST, or its errors' codes and messages.
fn with_queries(queries: &[(&str, &str)], src: &str) -> Result<String, Vec<(Code, String)>> {
    let mut schema = crate::lq::schema::Schema::core();
    for (name, text) in queries {
        schema.add_query(name, text);
    }
    let ids = fixture::ids();
    let (caller, params) = (Caller::default(), Params::new());
    let ctx = crate::lq::ctx::BindCtx {
        schema: &schema,
        ids: &ids,
        params: &params,
        caller: &caller,
    };
    let p = crate::lq::parser::parse_read(src, Default::default()).unwrap();
    crate::lq::bind::bind_read(&ctx, src, &p.tree)
        .map(|b| sexpr(Root::Query(&b.ast)))
        .map_err(|e| e.iter().map(|d| (d.code, d.message.clone())).collect())
}

/// A project named query that does not bind, or that is on a call cycle, is refused with its own text (E109), not as
/// an unknown name; each definition is bound once per bind, so a fan-out of calls stays linear, and a chain deeper
/// than eight definitions binds.
#[test]
fn project_named_queries_that_do_not_bind_or_cycle() {
    let e = with_queries(
        &[(
            "bad",
            "DEFINE QUERY bad() AS { MATCH (t:task) WHERE t.stauts = 'open' RETURN t }",
        )],
        "CALL bad() YIELD t RETURN t",
    )
    .unwrap_err();
    assert_eq!(e[0].0, Code::E109);
    assert!(
        e[0].1
            .starts_with("named query `bad` does not bind: E101 kind `task` has no field"),
        "{}",
        e[0].1
    );
    let cycle = [
        ("a", "DEFINE QUERY a() AS { CALL b() YIELD t RETURN t }"),
        ("b", "DEFINE QUERY b() AS { CALL a() YIELD t RETURN t }"),
    ];
    let e = with_queries(&cycle, "CALL a() YIELD t RETURN t").unwrap_err();
    assert_eq!(
        e[0],
        (
            Code::E109,
            "named query `a` is on a call cycle: a -> b -> a (QueryCycle)".to_string()
        )
    );
    let e = with_queries(
        &[("s", "DEFINE QUERY s() AS { CALL s() YIELD t RETURN t }")],
        "CALL s() YIELD t RETURN t",
    )
    .unwrap_err();
    assert_eq!(
        e[0].1,
        "named query `s` is on a call cycle: s -> s (QueryCycle)"
    );
    assert_eq!(
        with_queries(&[], "CALL nope() YIELD t RETURN t").unwrap_err()[0].1,
        "unknown function `nope`"
    );
    // q0 calls q1 twice, q1 calls q2 twice, ... q15 reads: 2^15 binds without the memo, 16 with it.
    let mut texts = Vec::new();
    for i in 0..15 {
        texts.push((
            format!("q{i}"),
            format!(
                "DEFINE QUERY q{i}() AS {{ CALL q{j}() YIELD t CALL q{j}() YIELD t AS u RETURN t }}",
                j = i + 1
            ),
        ));
    }
    texts.push((
        "q15".to_string(),
        "DEFINE QUERY q15() AS { MATCH (t:task) RETURN t }".to_string(),
    ));
    let refs: Vec<(&str, &str)> = texts
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let started = std::time::Instant::now();
    let s = with_queries(&refs, "CALL q0() YIELD t RETURN t").unwrap();
    assert!(s.contains("(CALL \"q0\" [] [(YIELD \"t\" 0)] _)"), "{s}");
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

/// The named-only safelist's refusal (E406, exit 6) is the first error, before any located error of the text.
#[test]
fn policy_refusals_come_first() {
    let safe = Caller {
        named_only: true,
        ..Caller::default()
    };
    let e = fixture::read_with(
        "MATCH (t:task) WHERE t.stauts = 'open' RETURN zz",
        &Params::new(),
        &safe,
    )
    .unwrap_err();
    assert_eq!(e[0].code, Code::E406);
    assert_eq!(e[1].code, Code::E101);
}

/// [LQ/envelope §4.3]: `<q>` is empty for a single hop, `{1,1}` included, in both display spellings.
#[test]
fn the_echo_prints_a_single_hop_plainly() {
    let gql = Caller {
        display: Spelling::Gql,
        ..Caller::default()
    };
    for caller in [Caller::default(), gql] {
        for src in [
            "MATCH (x:task)-[:BLOCKS*1]->(#93) RETURN x",
            "MATCH (x:task)-[:BLOCKS]->{1,1}(#93) RETURN x",
            "MATCH (x:task)((a)-[:BLOCKS]->(b)){1}(#93) RETURN x",
        ] {
            assert_eq!(
                *reads_as(src, &caller).last().unwrap(),
                "x BLOCKS #93 | x must finish before #93 starts",
                "{src}"
            );
        }
    }
}
