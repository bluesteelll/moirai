//! Robustness of the front end: any user text is refused, never a panic (names and durations with multi-byte
//! characters), operator and suffix chains of a million links on the caller's stack ([LQ/grammar-v1.ebnf §P.13]:
//! "keep their stacks on the heap"), and the deepest nesting the parser admits within the stack of a Windows main
//! thread.

use super::fixture;
use crate::lq::ast::{Expr, ExprKind};
use crate::lq::cast::{Root, encode, sexpr as cast_sexpr};
use crate::lq::ctx::{Caller, Params, Value};
use crate::lq::diag::{Code, Span};
use crate::lq::parser::{INLINE_NESTING, ParseOptions, nesting_bound, parse_read};
use crate::lq::printer::{Spelling, print_read};
use crate::lq::sexpr;

/// The codes of a read's bind errors (empty when it binds).
fn bind_codes(src: &str, params: &Params) -> Vec<Code> {
    match fixture::read_with(src, params, &Caller::default()) {
        Ok(_) => Vec::new(),
        Err(e) => e.iter().map(|d| d.code).collect(),
    }
}

/// The review's minimised case (`a0.é`) and its relatives: a `std.` prefix test that slices by bytes panicked when
/// byte 4 of a name fell inside a multi-byte character.
#[test]
fn call_names_with_multi_byte_characters() {
    for src in [
        "CALL a0.`é`()",
        "CALL ab.`é`()",
        "CALL `ab.é`()",
        "CALL `abcé`()",
        "CALL `é`.x()",
        "CALL std.`é`()",
        "MATCH (t:task) CALL ab.`é`() YIELD x RETURN t",
    ] {
        assert_eq!(bind_codes(src, &Params::new())[0], Code::E109, "{src}");
    }
    for name in ["a0.é", "ab.é", "abcé", "é", "std.é", "stdé", "std.", "ST"] {
        let e = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
            crate::lq::bind::bind_named(ctx, name)
        })
        .unwrap_err();
        assert_eq!((e[0].code, e[0].span), (Code::E109, None), "{name}");
    }
    // A `std.` prefix still works in any case.
    assert!(bind_codes("CALL STD.ready() YIELD t RETURN t", &Params::new()).is_empty());
}

/// A duration parameter's text (`k=v`, [LQ/std §2.2]) with a multi-byte last character is E110, not a panic; a
/// duration text no token could carry (a JSON IR `dur` node) is E003.
#[test]
fn duration_texts_with_multi_byte_characters() {
    let src = "RETURN $d + 1d AS t";
    for bad in ["5é", "é", "", "5", "5x", "-5d", "153722867280912931w"] {
        let p = Params::new().with("d", Value::Text(bad.into()));
        assert_eq!(bind_codes(src, &p), [Code::E110], "{bad:?}");
    }
    let p = Params::new().with("d", Value::Text("3d".into()));
    assert!(bind_codes(src, &p).is_empty());
    let text = "MATCH (t:task) RETURN t.updated_at + 1d AS at";
    let mut tree = parse_read(text, ParseOptions::default()).unwrap().tree;
    let crate::lq::ast::PartBody::Clauses { ret, .. } = &mut tree.query.parts[0].body else {
        panic!("a clause part");
    };
    let ExprKind::Arith(_, _, r) = &mut ret.items[0].expr.kind else {
        panic!("an arithmetic item");
    };
    **r = Expr::new(ExprKind::Dur("9é".into()), Span::new(37, 39));
    let e = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_read(ctx, text, &tree)
    })
    .unwrap_err();
    assert_eq!(e[0].code, Code::E003);
}

/// The links of each chain: a text of `n` links and what its tree must contain.
fn chain(kind: &str, n: usize) -> String {
    let mut s = String::with_capacity(n * 8 + 64);
    match kind {
        "+" => {
            s.push_str("MATCH (t:task) RETURN t.estimate");
            for _ in 0..n {
                s.push_str(" + 1");
            }
        }
        "OR" | "AND" => {
            s.push_str("MATCH (t:task) WHERE t.done");
            for _ in 0..n {
                s.push(' ');
                s.push_str(kind);
                s.push_str(" t.done");
            }
            s.push_str(" RETURN t");
        }
        "." => {
            s.push_str("RETURN NULL");
            for _ in 0..n {
                s.push_str(".a");
            }
            s.push_str(" AS x");
        }
        _ => {
            s.push_str("USE main");
            for _ in 0..n {
                s.push('~');
            }
            s.push_str(" MATCH (t:task) RETURN t");
        }
    }
    s
}

/// One chain of `n` links through the whole front end, one tree alive at a time: parse, print and parse back to the
/// same tree, render as an S-expression, clone and compare, bind, encode, render the C-AST, clone and compare it.
fn through_the_front_end(kind: &str, n: usize) {
    let src = chain(kind, n);
    let tree = {
        let p = parse_read(&src, ParseOptions::default())
            .unwrap_or_else(|e| panic!("{kind}: {}", super::show(&src, &e)));
        assert!(p.tokens.len() > n, "{kind}");
        p.tree
    };
    {
        let text = print_read(&tree, Spelling::Cypher);
        let again = parse_read(&text, ParseOptions::default()).unwrap().tree;
        assert!(again == tree, "{kind}: the printer property");
    }
    assert!(sexpr::read(&tree).len() > n * 4, "{kind}");
    {
        let copy = tree.clone();
        assert!(copy == tree, "{kind}: clone");
        assert!(!format!("{:?}", copy.query.parts[0]).is_empty());
    }
    let b = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
        crate::lq::bind::bind_read(ctx, &src, &tree)
    })
    .unwrap_or_else(|e| panic!("{kind}: {}", fixture::show(&src, &e)));
    drop(tree);
    assert!(encode(Root::Query(&b.ast)).len() > n * 2, "{kind}");
    assert!(cast_sexpr(Root::Query(&b.ast)).len() > n * 4, "{kind}");
    let ast = b.ast.clone();
    assert!(ast == b.ast, "{kind}: C-AST clone");
    assert!(!format!("{:?}", ast.first.use_).is_empty());
}

/// [LQ/grammar-v1.ebnf §P.13]: operator chains are loops, and so is a revision's suffix chain. A chain of a million
/// links — an arithmetic chain, a property chain, a suffix chain — goes through the whole front end on a test thread's
/// stack in an unoptimised build (recursion over such chains overflowed at about 30,000 links); so do the boolean
/// chains, at 200,000 links to keep the tier's time.
#[test]
fn chains_of_a_million_links() {
    for (kind, n) in [
        ("+", 1_000_000),
        (".", 1_000_000),
        ("~", 1_000_000),
        ("OR", 200_000),
        ("AND", 200_000),
    ] {
        through_the_front_end(kind, n);
    }
}

/// The estimate that decides where a text is parsed and bound ([`crate::lq::parser::for_text`]).
#[test]
fn the_nesting_bound_of_a_text() {
    assert_eq!(nesting_bound("MATCH (t:task) RETURN t"), 1);
    assert_eq!(nesting_bound("RETURN [[1], {a: (1 + 2)}]"), 3);
    assert_eq!(
        nesting_bound("RETURN '((((' + \"[[[\" + `{{{` // (((\n /* [[[ */ AS x"),
        0
    );
    assert_eq!(nesting_bound("RETURN 'a\\'(' AS x"), 0);
    assert_eq!(nesting_bound("MATCH (t) WHERE NOT not t.done RETURN t"), 3);
    assert_eq!(nesting_bound("RETURN CASE WHEN TRUE THEN 1 END AS x"), 1);
    assert_eq!(nesting_bound("RETURN notice, cases"), 0);
    assert_eq!(nesting_bound("RETURN 'unterminated (((("), 0);
    let deep = format!("RETURN {}1{}", "(".repeat(40), ")".repeat(40));
    assert_eq!(nesting_bound(&deep), 40);
}

/// The deepest texts the parser admits (64 levels of each kind of entry, [LQ/grammar-v1.ebnf §P.13]) and texts at the
/// in-place limit parse and bind from a thread with the stack of a Windows main thread (1 MiB): a deep text moves to
/// the front end's own thread.
#[test]
fn deep_texts_parse_and_bind_within_a_main_thread_stack() {
    let texts = |n: usize| {
        vec![
            format!(
                "MATCH (t:task) RETURN {}t.estimate{}",
                "(1 + ".repeat(n),
                ")".repeat(n)
            ),
            format!("MATCH (t:task) WHERE {}t.done RETURN t", "NOT ".repeat(n)),
            format!("RETURN {}1{} AS x", "[".repeat(n), "]".repeat(n)),
            format!(
                "MATCH (t:task) RETURN {}t.title{}",
                "toLower(".repeat(n),
                ")".repeat(n)
            ),
            format!(
                "MATCH (t:task) RETURN {}1{}",
                "CASE WHEN t.done THEN ".repeat(n),
                " END".repeat(n)
            ),
            format!("RETURN {}1{} AS m", "{a: ".repeat(n), "}".repeat(n)),
            format!(
                "MATCH (a:task) WHERE {}a.done{} RETURN a",
                "EXISTS { MATCH (a)-[:BLOCKS]->(b:task) WHERE ".repeat(n - 1),
                " }".repeat(n - 1)
            ),
        ]
    };
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            for n in [INLINE_NESTING as usize - 1, 64] {
                for src in texts(n) {
                    let p = parse_read(&src, ParseOptions::default())
                        .unwrap_or_else(|e| panic!("{}", super::show(&src, &e)));
                    let b = fixture::with_ctx(&Params::new(), &Caller::default(), |ctx| {
                        crate::lq::bind::bind_read(ctx, &src, &p.tree)
                    });
                    assert!(b.is_ok(), "{src}");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
