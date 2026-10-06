//! Tests of the relations, notices, ablations and `TX` statements beyond the core semantics: link states over a
//! simulated tree, the change feed against its definition, conflicts and staging refs, the catalog and runtime
//! relations, `--at`, W03, N01, N06, N12, the echo and lint ablations, and `TX` with `CALL tx.*`, `RESOLVE`, `MOVE`,
//! `REOPEN`, `DELETE`, created edges, counters and `IF TIP`.

use super::*;
use crate::api::{Data, Outcome};
use crate::links::{EnvGitCommit, EnvHead};
use crate::lq::diag::Code;
use crate::r4::tree::TreeOp;
use crate::value::{Algo, Nid};

fn n(i: u32) -> V {
    V::Node(Nid(i))
}

fn code(r: &Reply) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

const TREE: &str = "C:/work/moirai";
const HEAD: &str = "sha1:3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c";

fn in_tree() -> Ctx {
    Ctx {
        tree: Some(TREE.into()),
        ..orch()
    }
}

/// A store with three tasks, a tree holding `docs/api.md` committed at HEAD, `main` bound to it, and task `#2` linked
/// to line 3 of the file (file node `#5`, root node `#4`, anchor a1).
fn linked() -> Store {
    let mut s = S::base();
    s.ok(
        tx(vec![
            crate::suite::task("a", "Ship the Store API"),
            crate::suite::task("b", "Write the chapter"),
            crate::suite::task("c", "Write the examples"),
        ]),
        orch(),
    );
    s.ok(
        Cmd::EnvTree {
            tree: TREE.into(),
            volume: Some("C".into()),
            caps: None,
            ops: vec![
                TreeOp::Mkdir {
                    path: "docs".into(),
                    case_sensitive: None,
                },
                TreeOp::Write {
                    path: "docs/api.md".into(),
                    bytes: b"# API\n\n## Commands\n\nTx, Mutation and Apply.\n".to_vec(),
                    btime_ns: None,
                },
            ],
        },
        Ctx::default(),
    );
    s.ok(
        Cmd::EnvGit {
            repo: "moirai".into(),
            algo: Some(Algo::Sha1),
            commits: vec![EnvGitCommit {
                id: HEAD.into(),
                parents: Vec::new(),
                committer_time: 1_789_999_000,
                author_time: 1_789_999_000,
                tree: vec![(
                    "docs/api.md".into(),
                    "sha1:4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d".into(),
                )],
            }],
            refs: vec![("refs/heads/main".into(), Some(HEAD.into()))],
            heads: vec![(TREE.into(), EnvHead::Ref("refs/heads/main".into()))],
        },
        Ctx::default(),
    );
    s.ok(
        Cmd::WorktreeBind {
            dir: TREE.into(),
            ref_: "main".into(),
            replace: false,
        },
        orch(),
    );
    s.ok(
        Cmd::LinkFile {
            node: Target::Id(Nid(2)),
            specs: vec!["docs/api.md:3".into()],
            watch: None,
            planned: false,
            quote: None,
            end: None,
        },
        in_tree(),
    );
    s.st
}

/// Link states ([50 §2.6]; [LQ/std §2.8] item 3): an anchor's and a file's state, `none` for an unlinked node with W10
/// counting the rows it admitted, the presets after a raw move, and E302 without a tree.
#[test]
fn link_states_follow_the_tree() {
    let mut st = linked();
    let c = in_tree();
    let o = out(
        q_with(
            &mut st,
            "MATCH (n)-[a:AT]->(f) RETURN n, f, link_state(a), a.state, f.state, a.anchor, a.kind",
            Params::new(),
            Ablations::default(),
            &c,
        ),
        "links",
    );
    assert_eq!(
        o.rows,
        vec![vec![
            n(2),
            n(5),
            V::text("ok"),
            V::text("fresh"),
            V::text("ok"),
            V::text("a1"),
            // The capture of `docs/api.md:3` keeps its strongest unique selector, the quoted line ([40 §2.7]).
            V::text("quote")
        ]]
    );
    let w = out(
        q_with(
            &mut st,
            "MATCH (t:task) WHERE link_state(t) <> 'ok' RETURN t",
            Params::new(),
            Ablations::default(),
            &c,
        ),
        "w10",
    );
    assert_eq!(col_nodes(&w, 0), vec![1, 3]);
    let w10 = w
        .warnings
        .iter()
        .find(|x| x.code == Code::W10)
        .expect("W10");
    assert_eq!(w10.count, Some(2));
    let l = out(
        q_with(
            &mut st,
            "CALL links() YIELD node, anchor, file, state, next RETURN node, anchor, file, state, next",
            Params::new(),
            Ablations::default(),
            &c,
        ),
        "links()",
    );
    assert_eq!(
        l.rows,
        vec![vec![n(2), V::text("a1"), n(5), V::text("ok"), V::Absent]]
    );
    // A raw move: the file resolves by its id; the preset lists the link.
    st.run(
        &Cmd::EnvTree {
            tree: TREE.into(),
            volume: None,
            caps: None,
            ops: vec![TreeOp::Mv {
                from: "docs/api.md".into(),
                to: "docs/moved.md".into(),
            }],
        },
        &Ctx::default(),
    );
    let b = out(
        st.run(
            &Cmd::Query {
                input: QueryInput::Named("links_broken".into()),
                params: Params::new(),
                at: None,
                mode: Mode::Run,
                strict_gql: false,
                ablations: Ablations::default(),
            },
            &c,
        ),
        "links_broken",
    );
    assert_eq!(b.rows.len(), 1);
    assert_eq!(b.rows[0][2], V::text("moved-auto"));
    let e = q_with(
        &mut st,
        "MATCH (n)-[a:AT]->(f) RETURN link_state(a)",
        Params::new(),
        Ablations::default(),
        &orch(),
    );
    assert_eq!(code(&e), Some("E302"));
}

/// `changes()` and `relevant_to` ([LQ/std §2.15]): `std.changes` and `std.delta` equal the feed's definitions.
#[test]
fn the_change_feed_queries_equal_their_definitions() {
    let mut st = store();
    let o = out(
        st.run(
            &Cmd::Query {
                input: QueryInput::Named("changes".into()),
                params: Params::new()
                    .with("since", P::Text("s7".into()))
                    .with("all", P::Bool(true)),
                at: None,
                mode: Mode::Run,
                strict_gql: false,
                ablations: Ablations::default(),
            },
            &ctx(),
        ),
        "changes",
    );
    let main = st.dag.state_at(st.dag.live("main").unwrap().tip, &st.alloc);
    let want = crate::feed::changes(&st.feed, "main", 7, None, None, true, &main, &st.leases);
    assert_eq!(o.rows.len(), want.len());
    for (r, c) in o.rows.iter().zip(&want) {
        assert_eq!(r[0], V::Int(c.seq as i64));
        assert_eq!(r[1], V::text(c.ref_.clone()));
        assert_eq!(r[2], c.node.map_or(V::Absent, V::Node));
    }
    let d = out(
        st.run(
            &Cmd::Query {
                input: QueryInput::Named("delta".into()),
                params: Params::new()
                    .with("since", P::Text("s1".into()))
                    .with("agent", P::Text("dev#1".into())),
                at: None,
                mode: Mode::Run,
                strict_gql: false,
                ablations: Ablations::default(),
            },
            &ctx(),
        ),
        "delta",
    );
    let want = crate::feed::delta(&st.feed, 1, "dev#1", &main, &st.leases);
    assert_eq!(d.rows.len(), want.len());
}

/// `conflicts()` lists a landed conflict value; a staging ref is read-only with N02; `violations()` needs one.
#[test]
fn conflicts_and_staging_refs() {
    let mut st = store();
    let mut t = st.clone();
    let r = t.run(
        &Cmd::Merge {
            src: "main".into(),
            into: Some("lane/l5np".into()),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        &orch(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(
        &mut t,
        "USE lane/l5np CALL conflicts() YIELD key, node, class RETURN key, node, class",
    );
    assert_eq!(
        o.rows,
        vec![vec![V::text("#93.priority"), n(93), V::text("FieldEdit")]]
    );
    let c = rows(
        &mut t,
        "USE lane/l5np MATCH (t:task) WHERE t.conflicted RETURN t",
    );
    assert_eq!(col_nodes(&c, 0), vec![93]);
    let r = st.run(
        &Cmd::Merge {
            src: "main".into(),
            into: Some("lane/l5np".into()),
            policy: None,
            strict: Some(true),
            base: None,
            message: String::new(),
        },
        &orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged);
    let s = rows(
        &mut st,
        "USE merge/lane/l5np/from/main CALL violations() YIELD key RETURN key",
    );
    assert!(s.notices.iter().any(|x| x.code == Code::N02));
    let e = q(&mut st, "CALL violations() YIELD key RETURN key");
    assert_eq!(code(&e), Some("E302"));
}

/// The catalog, runtime and graph relations: `schema`, `schema_edges`, `markers`, `subtree`, `neighbors`, `blame`.
#[test]
fn catalog_and_graph_relations() {
    let mut st = store();
    let s = rows(
        &mut st,
        "CALL schema(kind: 'finding') YIELD field, type WHERE field = 'round' RETURN field, type",
    );
    assert_eq!(s.rows, vec![vec![V::text("round"), V::text("int")]]);
    let e = rows(
        &mut st,
        "CALL schema_edges() YIELD name, reverse_names WHERE name = 'BLOCKS' RETURN reverse_names",
    );
    assert_eq!(e.rows, vec![vec![V::List(vec![V::text("BLOCKED_BY")])]]);
    let t = rows(
        &mut st,
        "CALL subtree(#9) YIELD node, depth, parent, position RETURN node, depth, parent, position",
    );
    assert_eq!(
        t.rows,
        vec![
            vec![n(9), V::Int(0), V::Absent, V::Int(0)],
            vec![n(12), V::Int(1), n(9), V::Int(1)],
            vec![n(14), V::Int(1), n(9), V::Int(2)],
            vec![n(17), V::Int(1), n(9), V::Int(3)]
        ]
    );
    let nb = rows(
        &mut st,
        "CALL neighbors(#17, depth: 1, types: ['BLOCKS']) YIELD node, dir RETURN node, dir",
    );
    assert_eq!(
        nb.rows,
        vec![vec![n(14), V::text("in")], vec![n(51), V::text("out")]]
    );
    let b = rows(
        &mut st,
        "CALL blame(#136) YIELD aspect, seq WHERE aspect = 'status' RETURN seq",
    );
    assert_eq!(b.rows, vec![vec![V::Int(2)]]);
    let m = rows(&mut st, "CALL markers() YIELD n RETURN count(*) AS c");
    assert_eq!(m.rows, vec![vec![V::Int(0)]]);
}

/// `--at` ([50 §3.9] item 1), W03, N01, N06 and N12 ([LQ/errors §5.6]).
#[test]
fn views_and_notices() {
    let mut st = store();
    let r = st.run(
        &Cmd::Query {
            input: QueryInput::Lq("MATCH (t {id: #136}) RETURN t.status".into()),
            params: Params::new(),
            at: Some("s1".into()),
            mode: Mode::Run,
            strict_gql: false,
            ablations: Ablations::default(),
        },
        &ctx(),
    );
    let o = out(r, "--at");
    assert_eq!(o.rows[0][0].as_str(), Some("open"));
    // W03: a cancelled task read through `done`.
    let c = tx_lq(
        &mut st,
        "TX { SET #52.status = 'cancelled' }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(c.outcome, Outcome::Ok, "{:?}", c.error);
    let w = rows(&mut st, "MATCH (t:task) WHERE t.done RETURN t");
    assert_eq!(col_nodes(&w, 0), vec![52]);
    assert!(
        w.warnings
            .iter()
            .any(|x| x.code == Code::W03 && x.message.starts_with("t.done"))
    );
    // N01: a literal id of a deleted node; N06: an id created on another branch; N12 through a parameter.
    let d = tx_lq(
        &mut st,
        "TX { DELETE #142 REASON 'dup' }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(d.outcome, Outcome::Ok, "{:?}", d.error);
    let o = rows(&mut st, "MATCH (x {id: #142}) RETURN x");
    assert!(o.rows.is_empty());
    assert!(o.notices.iter().any(|x| x.code == Code::N01));
    let tomb = rows(
        &mut st,
        "MATCH (x:DELETED {id: #142}) RETURN x.deleted_reason, x.kind",
    );
    assert_eq!(tomb.rows, vec![vec![V::text("dup"), V::text("note")]]);
    let r = tx_lq(
        &mut st,
        "TX ON lane/l5np { CREATE (n:note {title: 'lane only'}) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let lane_only = st.next_id - 1;
    let o = rows(&mut st, &format!("MATCH (x {{id: #{lane_only}}}) RETURN x"));
    assert!(o.rows.is_empty());
    assert!(o.notices.iter().any(|x| x.code == Code::N06));
}

/// The ablations of [50 §7.4] item 7 the binder's outputs carry: the reading echo, W07 and suggestions off.
#[test]
fn echo_lint_and_suggestion_ablations() {
    let mut st = store();
    let text = "MATCH (t:task) WHERE t.status = 'open' AND NOT (t)<-[:BLOCKS]-() RETURN t";
    let on = rows(&mut st, text);
    assert!(on.warnings.iter().any(|x| x.code == Code::W07));
    let off = out(
        q_with(
            &mut st,
            text,
            Params::new(),
            Ablations {
                no_w07: true,
                ..Ablations::default()
            },
            &ctx(),
        ),
        "no W07",
    );
    assert!(!off.warnings.iter().any(|x| x.code == Code::W07));
    let echo = "MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b";
    assert!(!rows(&mut st, echo).reads.is_empty());
    let silent = out(
        q_with(
            &mut st,
            echo,
            Params::new(),
            Ablations {
                no_echo: true,
                ..Ablations::default()
            },
            &ctx(),
        ),
        "no echo",
    );
    assert!(silent.reads.is_empty());
    // The display spelling of a quantifier in the echo: Cypher by default, GQL under the ablation.
    let hops = "MATCH (#51)-[:BLOCKED_BY*1..3]->(b) RETURN b";
    let spelled = |st: &mut Store, gql: bool| -> String {
        out(
            q_with(
                st,
                hops,
                Params::new(),
                Ablations {
                    gql_display: gql,
                    ..Ablations::default()
                },
                &ctx(),
            ),
            hops,
        )
        .reads
        .join(
            "
",
        )
    };
    let (cypher, gql) = (spelled(&mut st, false), spelled(&mut st, true));
    assert!(cypher.contains("*1..3"), "{cypher}");
    assert!(gql.contains("{1,3}") && !gql.contains("*1..3"), "{gql}");
    let diag = |ab: Ablations| {
        let mut req = crate::lq::eval::QueryReq::lq("MATCH (t:task) WHERE t.open RETURN t");
        req.ablations = ab;
        match st.query_full(&req, &ctx()) {
            Err(crate::lq::eval::QueryError::Text(d)) => d[0].clone(),
            other => panic!("{other:?}"),
        }
    };
    let with = diag(Ablations::default());
    assert_eq!(with.code, Code::E101);
    assert!(with.inline.is_some() || with.help.is_some() || !with.suggest.is_empty());
    let without = diag(Ablations {
        no_suggestions: true,
        ..Ablations::default()
    });
    assert_eq!(without.code, Code::E101);
    assert!(without.inline.is_none() && without.help.is_none() && without.suggest.is_empty());
}

/// `TX` statements: `CALL tx.claim` with its yields visible to later statements, `MOVE`, `REOPEN`, `DELETE` of an
/// edge, created edges with `UNDER`, `RESOLVE` by a query's keys, and `IF TIP`.
#[test]
fn tx_statements_compile_to_the_kernel() {
    let mut st = store();
    let dev = Ctx {
        agent: Some("dev#2".into()),
        ..Default::default()
    };
    let r = tx_lq(
        &mut st,
        "TX { CALL tx.claim(ids: [#52], ttl: '1h') YIELD lease }",
        Params::new(),
        &dev,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(r.yields[0].proc, "tx.claim");
    let c = ctx();
    let r = tx_lq(
        &mut st,
        "TX { CREATE (n:task {title: 'Probe ordering', priority: 1}) UNDER #9; CREATE (#14)-[:BLOCKS]->(n); MOVE #12 UNDER #88 LAST }",
        Params::new(),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let new = st.next_id - 1;
    let o = rows(
        &mut st,
        "MATCH (#14)-[:BLOCKS]->(x) WHERE x.parent = #9 RETURN x, x.priority",
    );
    assert_eq!(
        o.rows,
        vec![vec![n(17), V::Int(2)], vec![n(new), V::Int(1)]]
    );
    let p = rows(&mut st, "MATCH (t {id: #12}) RETURN t.parent");
    assert_eq!(p.rows, vec![vec![n(88)]]);
    let r = tx_lq(
        &mut st,
        "TX { MATCH (a {id: #14})-[e:BLOCKS]->(b {id: #17}) EXPECT 1 DELETE e }",
        Params::new(),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert!(
        rows(&mut st, "MATCH (#14)-[:BLOCKS]->(x {id: #17}) RETURN x")
            .rows
            .is_empty()
    );
    // REOPEN a done task.
    tx_lq(
        &mut st,
        "TX { SET #20.status = 'in_progress' }",
        Params::new(),
        &c,
    );
    let d = tx_lq(&mut st, "TX { SET #92.done = true }", Params::new(), &c);
    assert_eq!(d.outcome, Outcome::Ok, "{:?}", d.error);
    let r = tx_lq(
        &mut st,
        "TX { REOPEN #92 REASON 'regressed' }",
        Params::new(),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(
        &mut st,
        "MATCH (t {id: #92}) RETURN t.status, t.reopen_count",
    );
    assert_eq!(o.rows[0][0].as_str(), Some("open"));
    // IF TIP: a stale tip refuses with E402.
    let e = tx_lq(
        &mut st,
        "TX IF TIP s1 { SET #52.priority = 0 }",
        Params::new(),
        &c,
    );
    assert_eq!(code(&e), Some("E402"));
    // RESOLVE by a query's keys on a landed conflict.
    let r = st.run(
        &Cmd::Merge {
            src: "main".into(),
            into: Some("lane/l5np".into()),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        &orch(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let r = tx_lq(
        &mut st,
        "TX ON lane/l5np { RESOLVE (CALL conflicts() YIELD key RETURN key) EXPECT >= 1 TAKE THEIRS }",
        Params::new(),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(
        &mut st,
        "USE lane/l5np MATCH (t {id: #93}) RETURN t.conflicted, t.priority",
    );
    assert_eq!(o.rows, vec![vec![V::Bool(false), V::Int(3)]]);
    assert!(matches!(r.data, Data::None));
}

/// `file()` and `staleness()` ([50 §2.6]): the artifact at a path, and a pinned commit against the tree's HEAD.
#[test]
fn file_and_staleness() {
    let mut st = linked();
    let c = in_tree();
    let f = out(
        q_with(
            &mut st,
            "MATCH (f:artifact) WHERE f = file('docs/api.md') RETURN f",
            Params::new(),
            Ablations::default(),
            &c,
        ),
        "file",
    );
    assert_eq!(col_nodes(&f, 0), vec![5]);
    let r = tx_lq(
        &mut st,
        "TX { CREATE (m:measurement {title: 'p99 append', measured_on: $c}) }",
        Params::new().with("c", P::Text(HEAD.into())),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let s = out(
        q_with(
            &mut st,
            "MATCH (m:measurement) RETURN staleness(m)",
            Params::new(),
            Ablations::default(),
            &c,
        ),
        "staleness",
    );
    assert_eq!(s.rows, vec![vec![V::text("fresh")]]);
}

/// The converter's trees ([LQ/json-ir]): `Query` with `ir` and `Tx` with `ir` run as their LQ texts do.
#[test]
fn ir_trees_run_as_their_texts() {
    let mut st = store();
    let text = "MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b";
    let tree = crate::lq::parser::parse_read(text, crate::lq::parser::ParseOptions::default())
        .expect("parses")
        .tree;
    let r = st.run(
        &Cmd::Query {
            input: QueryInput::Ir(Box::new(tree)),
            params: Params::new(),
            at: None,
            mode: Mode::Run,
            strict_gql: false,
            ablations: Ablations::default(),
        },
        &ctx(),
    );
    let o = out(r, "ir");
    assert_eq!(col_nodes(&o, 0), vec![12, 17]);
    let t = crate::lq::parser::parse_write(
        "TX { SET #52.priority = 0 }",
        crate::lq::parser::ParseOptions::default(),
    )
    .expect("parses")
    .tree;
    let r = st.run(
        &Cmd::TxIr {
            tree: Box::new(t),
            params: Params::new(),
            message: "via ir".into(),
            if_targets: None,
        },
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let seq = r.rev_new.expect("a commit");
    assert_eq!(st.dag.commits[&seq].message, "via ir");
}
