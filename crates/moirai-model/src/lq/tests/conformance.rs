//! The conformance fixtures of `fixtures/lq/` (WP-22, [50 §8.3]): the token, AST and error cases that WP-93a's
//! acceptance names, and the standard-library sources of `fixtures/lq/std/` ([LQ/std §1]).
//!
//! No chapter fixes the files' layout yet (see the WP-93a spec findings); the runner reads this one, which carries
//! exactly the formats the chapters define:
//!
//! - A case is a file `<base>.lq` anywhere under `fixtures/lq/` except `std/`: the LQ text, decoded by
//!   [LQ/lexical §2.1]. `<base>`'s last dot-separated segments choose the entry ([LQ/grammar-v1.ebnf §P.1]): `tx` for
//!   `write_input`, `def` for `define_stmt`, `read_input` otherwise; `strict` adds the strict-GQL spelling mode
//!   (`name.tx.strict.lq`).
//! - Beside it, one or more expectations: `<base>.tokens`, the token stream of [LQ/lexical §11] (compared byte for
//!   byte); `<base>.sast`, the S-expression of [LQ/canonical-ast §4.2] (compared by §4.1); `<base>.err`, the first
//!   error as `<code> <line> <col>`, or `<code>` alone for an unlocated one (a parse error, or when the text parses, a
//!   bind error); `<base>.cast`, the binding context of §9 (one item per line: `schema core`, `node <N> <32 hex>`,
//!   `commit <seq> <64 hex>`, `param <name> <type> <value>`), an empty line, the S-expression of §4.3, and optionally
//!   `hex` lines holding the encoding (§6) and a `hash` line holding `H`, the first 16 bytes of its BLAKE3-256 (§7.1).
//!   A `.err` case binds in the context of `<base>.ctx` (the same lines) when that file exists.
//!
//! The directory is WP-22's first output (PLAN §3.3: `lq/`, then WP-21's `canonical/` and `r4/`, WP-20's `hex/`, then
//! `gt10/`). Until it exists the check has nothing to read; once any later directory exists, a missing `fixtures/lq`
//! fails the check, and so does a `fixtures/lq` without a case.

use crate::lq::cast::{Root, encode, sexpr as cast_sexpr};
use crate::lq::ctx::{BindCtx, Caller, MapIds, Params, Value};
use crate::lq::diag::{Diag, line_col};
use crate::lq::lexer::decode;
use crate::lq::parser::{ParseOptions, parse_define, parse_read, parse_write, token_stream_text};
use crate::lq::schema::Schema;
use crate::lq::{bind, sexpr};
use std::path::{Path, PathBuf};

/// `fixtures/` of the repository.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The fixture directories R-FIX writes after `lq/` (PLAN §3.3).
const LATER: [&str; 4] = ["canonical", "r4", "hex", "gt10"];

/// The start symbol of a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Read,
    Write,
    Define,
}

/// A parsed case: its tree, ready for the S-expression renderer and the binder.
enum Tree {
    Read(crate::lq::ast::Read),
    Write(crate::lq::ast::Tx),
    Define(crate::lq::ast::Define),
}

/// The binding context of a C-AST case ([LQ/canonical-ast §9]).
struct Context {
    schema: Schema,
    ids: MapIds,
    params: Params,
}

impl Context {
    /// The context of §9's lines; with no line, the core schema and an empty store.
    fn parse(lines: &[&str]) -> Result<Context, String> {
        let mut schema = None;
        let mut ids = MapIds::new();
        let mut params = Params::new();
        for line in lines {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["schema", "core"] => schema = Some(Schema::core()),
                ["node", n, hex, rest @ ..] if rest.len() <= 1 => {
                    let n = n
                        .parse()
                        .map_err(|_| format!("bad node number in {line:?}"))?;
                    ids.node(n, unhex(hex)?, rest.first().copied());
                }
                ["commit", s, hex, rest @ ..] if rest.len() <= 1 => {
                    let s = s
                        .parse()
                        .map_err(|_| format!("bad sequence number in {line:?}"))?;
                    ids.commit(s, unhex(hex)?, rest.first().copied().unwrap_or("main"));
                }
                ["param", name, ty, ..] => {
                    let value = line
                        .splitn(4, char::is_whitespace)
                        .nth(3)
                        .map(str::trim)
                        .ok_or_else(|| format!("no value in {line:?}"))?;
                    params = params.with(name, param_value(ty, value));
                }
                _ => return Err(format!("not a binding-context line: {line:?}")),
            }
        }
        Ok(Context {
            schema: schema.unwrap_or_else(Schema::core),
            ids,
            params,
        })
    }
}

/// A parameter's value from its `k=v` text ([LQ/std §2.2]): `null` is NULL; an `int`, `float` or `bool` text its
/// number or truth value; everything else the text, which the binder converts by the use site's type.
fn param_value(ty: &str, v: &str) -> Value {
    match (ty, v) {
        (_, "null") => Value::Null,
        ("int", _) => v.parse().map_or_else(|_| Value::Text(v.into()), Value::Int),
        ("float", _) => v
            .parse()
            .map_or_else(|_| Value::Text(v.into()), Value::Float),
        ("bool", "true") => Value::Bool(true),
        ("bool", "false") => Value::Bool(false),
        _ => Value::Text(v.into()),
    }
}

/// Bytes from lower-case hex digits.
fn unhex<const N: usize>(h: &str) -> Result<[u8; N], String> {
    let v = unhex_vec(h)?;
    v.try_into().map_err(|_| format!("{h:?} is not {N} bytes"))
}

fn unhex_vec(h: &str) -> Result<Vec<u8>, String> {
    let h: String = h.split_whitespace().collect();
    if !h.len().is_multiple_of(2) || !h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(format!("{h:?} is not lower-case hex"));
    }
    Ok((0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0))
        .collect())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The first error as a fixture writes it: `<code> <line> <col>`, or `<code>` when unlocated.
fn first_error(src: &str, e: &[Diag]) -> String {
    let d = &e[0];
    match d.span {
        Some(s) => {
            let (l, c) = line_col(src, s.start);
            format!("{} {l} {c}", d.code)
        }
        None => d.code.to_string(),
    }
}

/// The entry and mode a case's base name selects.
fn entry_of(base: &str) -> (Entry, ParseOptions) {
    let mut entry = Entry::Read;
    let mut opts = ParseOptions::default();
    for seg in base.rsplit('.') {
        match seg {
            "tx" => entry = Entry::Write,
            "def" => entry = Entry::Define,
            "strict" => opts.strict_gql = true,
            _ => break,
        }
    }
    (entry, opts)
}

/// Parses a case's text: its tree and token stream, or its diagnostics.
fn parse(src: &str, entry: Entry, opts: ParseOptions) -> Result<(Tree, String), Vec<Diag>> {
    Ok(match entry {
        Entry::Read => {
            let p = parse_read(src, opts)?;
            (Tree::Read(p.tree), token_stream_text(&p.tokens))
        }
        Entry::Write => {
            let p = parse_write(src, opts)?;
            (Tree::Write(p.tree), token_stream_text(&p.tokens))
        }
        Entry::Define => {
            let p = parse_define(src, opts)?;
            (Tree::Define(p.tree), token_stream_text(&p.tokens))
        }
    })
}

/// The S-AST S-expression of a tree.
fn sast(t: &Tree) -> String {
    match t {
        Tree::Read(r) => sexpr::read(r),
        Tree::Write(t) => sexpr::tx(t),
        Tree::Define(d) => sexpr::define(d),
    }
}

/// Binds a tree in a context: its C-AST S-expression and encoding, or its diagnostics.
fn bind_tree(src: &str, t: &Tree, cx: &Context) -> Result<(String, Vec<u8>), Vec<Diag>> {
    let caller = Caller::default();
    let ctx = BindCtx {
        schema: &cx.schema,
        ids: &cx.ids,
        params: &cx.params,
        caller: &caller,
    };
    let render = |root: Root<'_>| (cast_sexpr(root), encode(root));
    Ok(match t {
        Tree::Read(r) => render(Root::Query(&bind::bind_read(&ctx, src, r)?.ast)),
        Tree::Write(w) => render(Root::Tx(&bind::bind_write(&ctx, src, w)?.ast)),
        Tree::Define(d) => render(Root::Define(&bind::bind_define(&ctx, src, d)?.ast)),
    })
}

/// The expectations of one case, by file suffix.
#[derive(Default)]
struct Expect {
    tokens: Option<String>,
    sast: Option<String>,
    err: Option<String>,
    cast: Option<String>,
    ctx: Option<String>,
}

/// Checks one case: its text, the entry its base name selects, and its expectations.
fn check(base: &str, bytes: &[u8], x: &Expect) -> Result<(), String> {
    let (entry, opts) = entry_of(base);
    let src = decode(bytes).map_err(|d| format!("{} (decoding)", d.code))?;
    if x.tokens.is_none() && x.sast.is_none() && x.err.is_none() && x.cast.is_none() {
        return Err("no expectation file".into());
    }
    let parsed = parse(src, entry, opts);
    if let Some(want) = &x.err {
        let want = want.trim();
        let got = match &parsed {
            Err(e) => first_error(src, e),
            Ok((tree, _)) => {
                let lines: Vec<&str> = x.ctx.as_deref().unwrap_or("").lines().collect();
                let cx = Context::parse(&lines)?;
                match bind_tree(src, tree, &cx) {
                    Err(e) => first_error(src, &e),
                    Ok(_) => return Err(format!("parses and binds; the fixture expects {want}")),
                }
            }
        };
        if got != want {
            return Err(format!("first error {got}, the fixture expects {want}"));
        }
    }
    if x.tokens.is_none() && x.sast.is_none() && x.cast.is_none() {
        return Ok(());
    }
    let (tree, tokens) = parsed.map_err(|e| format!("does not parse: {}", first_error(src, &e)))?;
    if let Some(want) = &x.tokens
        && tokens != *want
    {
        return Err(format!("token stream:\n{tokens}the fixture has:\n{want}"));
    }
    if let Some(want) = &x.sast {
        let got = sast(&tree);
        if !sexpr::same(&got, want) {
            return Err(format!("S-AST {got}, the fixture has {}", want.trim()));
        }
    }
    if let Some(text) = &x.cast {
        let lines: Vec<&str> = text.lines().collect();
        let blank = lines.iter().position(|l| l.trim().is_empty());
        let (context, rest) = match blank {
            Some(i) if !lines[0].trim_start().starts_with('(') => (&lines[..i], &lines[i + 1..]),
            _ => (&lines[..0], &lines[..]),
        };
        let cx = Context::parse(context)?;
        let tail = rest
            .iter()
            .position(|l| l.starts_with("hex ") || l.starts_with("hash "))
            .unwrap_or(rest.len());
        let want_cast = rest[..tail].join("\n");
        let hex_want: String = rest[tail..]
            .iter()
            .filter_map(|l| l.strip_prefix("hex "))
            .collect::<Vec<_>>()
            .join(" ");
        let hash_want = rest[tail..]
            .iter()
            .find_map(|l| l.strip_prefix("hash "))
            .map(str::trim);
        let (got, bytes) = bind_tree(src, &tree, &cx)
            .map_err(|e| format!("does not bind: {}", first_error(src, &e)))?;
        if !sexpr::same(&got, &want_cast) {
            return Err(format!("C-AST {got}, the fixture has {want_cast}"));
        }
        if !hex_want.trim().is_empty() && bytes != unhex_vec(&hex_want)? {
            return Err(format!(
                "encoding {}, the fixture has {hex_want}",
                hex(&bytes)
            ));
        }
        if let Some(h) = hash_want {
            let got = hex(&blake3::hash(&bytes).as_bytes()[..16]);
            if got != h {
                return Err(format!("H {got}, the fixture has {h}"));
            }
        }
    }
    Ok(())
}

/// Every file under `dir`, recursively, in path order.
fn files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// Reads an expectation file when it exists.
fn read_opt(p: &Path) -> Result<Option<String>, String> {
    match std::fs::read(p) {
        Ok(b) => String::from_utf8(b)
            .map(Some)
            .map_err(|_| format!("{} is not UTF-8", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// Runs every case under `lq` (not `lq/std/`): how many there are, or the failures, each `path: why`.
fn run_cases(lq: &Path) -> Result<usize, Vec<String>> {
    let mut all = Vec::new();
    files(lq, &mut all).map_err(|e| vec![format!("{}: {e}", lq.display())])?;
    let std_dir = lq.join("std");
    let mut cases = 0;
    let mut failures = Vec::new();
    for p in all
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "lq") && !p.starts_with(&std_dir))
    {
        cases += 1;
        let base = p.with_extension("");
        let base_name = base
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sibling = |suffix: &str| {
            let mut s = base.clone().into_os_string();
            s.push(suffix);
            PathBuf::from(s)
        };
        let result = (|| -> Result<(), String> {
            let x = Expect {
                tokens: read_opt(&sibling(".tokens"))?,
                sast: read_opt(&sibling(".sast"))?,
                err: read_opt(&sibling(".err"))?,
                cast: read_opt(&sibling(".cast"))?,
                ctx: read_opt(&sibling(".ctx"))?,
            };
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            check(&base_name, &bytes, &x)
        })();
        if let Err(e) = result {
            failures.push(format!("{}: {e}", p.display()));
        }
    }
    if failures.is_empty() {
        Ok(cases)
    } else {
        Err(failures)
    }
}

#[test]
fn the_conformance_fixtures_of_fixtures_lq() {
    let root = fixtures();
    let lq = root.join("lq");
    if !lq.is_dir() {
        let later: Vec<&str> = LATER
            .iter()
            .copied()
            .filter(|d| root.join(d).is_dir())
            .collect();
        assert!(
            later.is_empty(),
            "fixtures/lq is missing although fixtures/{later:?} exist, and WP-22's lq/ part comes first (PLAN §3.3)"
        );
        return;
    }
    match run_cases(&lq) {
        Ok(cases) => assert!(cases > 0, "fixtures/lq holds no case"),
        Err(failures) => panic!(
            "{} conformance cases fail:\n{}",
            failures.len(),
            failures.join("\n")
        ),
    }
}

/// The runner over a directory tree like WP-22's: cases found in subdirectories, entries chosen by base names,
/// expectations read beside their cases, `std/` left to its own check, and a failing case reported by its path.
#[test]
fn the_runner_walks_a_fixture_tree() {
    let dir = std::env::temp_dir().join(format!("moirai-model-lq-fixtures-{}", std::process::id()));
    let result = std::panic::catch_unwind(|| {
        let write = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write("tokens/one.lq", "RETURN 1 AS x");
        write(
            "tokens/one.tokens",
            "KW RETURN\nINT 1\nKW AS\nNAME x\nEOF -\n",
        );
        write("errors/e006.lq", "TX { DELETE #1 }");
        write("errors/e006.err", "E006 1 1\n");
        write("errors/empty.tx.lq", "TX { }");
        write("errors/empty.tx.err", "E009 1 6\n");
        write("std/ignored.lq", "not a case");
        assert_eq!(run_cases(&dir), Ok(3));
        write("errors/wrong.lq", "RETURN 1 AS x");
        write("errors/wrong.err", "E001 1 1\n");
        let failures = run_cases(&dir).unwrap_err();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("wrong.lq"), "{failures:?}");
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// [LQ/std §1]: every `fixtures/lq/std/<name>.lq` is the source of `std.<name>`: it parses with start symbol
/// `define_stmt`, names `<name>`, and binds to the C-AST of the model's own standard library.
#[test]
fn the_standard_library_sources_of_fixtures_lq_std() {
    let dir = fixtures().join("lq/std");
    if !dir.is_dir() {
        return;
    }
    let mut all = Vec::new();
    files(&dir, &mut all).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut failures = Vec::new();
    for p in all
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "lq"))
    {
        let name = p
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
            let src = decode(&bytes).map_err(|d| format!("{} (decoding)", d.code))?;
            let d = parse_define(src, ParseOptions::default())
                .map_err(|e| format!("does not parse: {}", first_error(src, &e)))?
                .tree;
            if d.name.text != name {
                return Err(format!("defines {:?}", d.name.text));
            }
            let cx = Context::parse(&[])?;
            let (_, bytes) = bind_tree(src, &Tree::Define(d), &cx)
                .map_err(|e| format!("does not bind: {}", first_error(src, &e)))?;
            let ours = crate::lq::catalog::std_query(&name)
                .ok_or_else(|| "the model has no such standard query".to_string())?;
            if bytes != encode(Root::Define(&ours.cast)) {
                return Err("binds to another C-AST than the model's".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            failures.push(format!("{}: {e}", p.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ----- the runner on the examples of the chapters -------------------------------------------------------------------

fn case(base: &str, text: &str, x: Expect) -> Result<(), String> {
    check(base, text.as_bytes(), &x)
}

/// [LQ/lexical §11.2]'s examples as token cases.
#[test]
fn token_cases_of_lexical_11_2() {
    let range = "KW CALL\nNAME log\nP (\nREF main\nRANGE ..\nREF lane/l5np\nP )\nKW YIELD\nNAME commit\nP ,\nNAME actor\nP ,\nNAME message\nEOF -\n";
    case(
        "range-2dot",
        "CALL log(main..lane/l5np) YIELD commit, actor, message",
        Expect {
            tokens: Some(range.into()),
            ..Expect::default()
        },
    )
    .unwrap();
    let use_at = "KW USE\nREF main\nSUF @2026-09-25T10:00:00Z\nKW MATCH\nP (\nNAME s1\nP :\nNAME doc\nP )\nKW WHERE\nNAME s1\nKW IN\nNAME subtree\nP (\nNODE 130\nP )\nKW RETURN\nNAME s1\nEOF -\n";
    case(
        "use-at",
        "USE main@2026-09-25T10:00Z MATCH (s1:doc) WHERE s1 IN subtree(#130) RETURN s1",
        Expect {
            tokens: Some(use_at.into()),
            ..Expect::default()
        },
    )
    .unwrap();
    let card7 = "KW TX\nKW ON\nREF lane/l5np\nKW LEASE\nSTR \"L-18\"\nP {\nKW MATCH\nP (\nNAME t\nP {\nNAME id\nP :\nNODE 89\nP }\nP )\nKW WHERE\nNAME t\nP .\nNAME status\nP =\nSTR \"in_progress\"\nKW EXPECT\nINT 1\nKW SET\nNAME t\nP .\nNAME done\nP =\nKW TRUE\nP }\nEOF -\n";
    case(
        "card-7.tx",
        "TX ON lane/l5np LEASE 'L-18' { MATCH (t {id: #89}) WHERE t.status = 'in_progress' EXPECT 1 SET t.done = true }",
        Expect {
            tokens: Some(card7.into()),
            ..Expect::default()
        },
    )
    .unwrap();
    // A wrong stream fails.
    assert!(
        case(
            "range-2dot",
            "CALL log(main..lane/l5np) YIELD commit",
            Expect {
                tokens: Some(range.into()),
                ..Expect::default()
            },
        )
        .is_err()
    );
}

/// [LQ/canonical-ast §4.4]'s example as an S-AST case and as a C-AST case with the encoding of §6.5.
#[test]
fn ast_cases_of_canonical_ast_4_4() {
    let text = "MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5";
    let sast = r#"(read run
 (query
  [(part _
    [(match false _
      [(path (npat "t" ["task"] [] _) [])]
      (and (cmp = (prop (ident "t") "status") (str "open"))
           (cmp <= (prop (ident "t") "priority") (int 1))))]
    (return false false [(item (ident "t") _)] [] [(sort (prop (ident "t") "priority") asc)] (int 5))
    _)]
  []))"#;
    let bytes = "10 00 00 00 6d 6f 69 72 61 69 2d 6c 71 2d 61 73 74 2d 76 31 01 00 01 02 00 03 01 00 00 00 10 00
01 00 00 00 20 21 01 00 00 00 00 01 00 00 00 04 00 00 00 74 61 73 6b 00 00 00 00 00 00 00 00 00
01 31 33 01 3a 3b 00 00 00 00 06 00 00 00 73 74 61 74 75 73 58 04 00 00 00 6f 70 65 6e 33 04 3a
3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 52 01 00 00 00 00 00 00 00 14 00 00 01 00 00
00 16 3b 00 00 00 00 00 01 00 00 00 18 3a 3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 00
01 52 05 00 00 00 00 00 00 00 00 00 00 00";
    let enc = unhex_vec(bytes).unwrap();
    let h = hex(&blake3::hash(&enc).as_bytes()[..16]);
    let cast = format!(
        "schema core\n\n(QUERY (PART _ (CLAUSES [(MATCH false [(PATH (NODEP 0 [\"task\"] [] _) [])] (AND (CMP = (PROP (VAR 0) \"status\") (ENUM \"open\")) (CMP <= (PROP (VAR 0) \"priority\") (INT 1))))] (RETURN false false [(RITEM (VAR 0) _)] [(SORT (PROP (VAR 0) \"priority\") false)] (INT 5)))) [])\n{}\nhash {h}\n",
        bytes
            .lines()
            .map(|l| format!("hex {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    case(
        "example-4-4",
        text,
        Expect {
            sast: Some(sast.into()),
            cast: Some(cast.clone()),
            ..Expect::default()
        },
    )
    .unwrap();
    // The lower-case spelling of §4.4 gives the same C-AST, encoding and hash.
    case(
        "example-4-4-spelling",
        "match (x:Task) where x.status = open and x.priority <= 'P1' return x order by x.priority asc limit 5",
        Expect {
            cast: Some(cast.clone()),
            ..Expect::default()
        },
    )
    .unwrap();
    // A wrong hash fails.
    let wrong = cast.replace(&format!("hash {h}"), &format!("hash {}", "0".repeat(32)));
    assert!(
        case(
            "example-4-4",
            text,
            Expect {
                cast: Some(wrong),
                ..Expect::default()
            },
        )
        .is_err()
    );
}

/// Error cases: a parse error, a bind error in a §9 context, an unlocated error, and the entry and mode a base name
/// selects.
#[test]
fn error_cases_and_contexts() {
    let err = |base: &str, text: &str, want: &str, ctx: Option<&str>| {
        case(
            base,
            text,
            Expect {
                err: Some(format!("{want}\n")),
                ctx: ctx.map(str::to_string),
                ..Expect::default()
            },
        )
    };
    err(
        "null-cmp",
        "MATCH (a) WHERE a.x = NULL RETURN a",
        "E118 1 17",
        None,
    )
    .unwrap();
    err("tx-in-q", "TX { DELETE #1 }", "E006 1 1", None).unwrap();
    err("q-in-tx.tx", "MATCH (t) RETURN t", "E001 1 1", None).unwrap();
    err(
        "strict.strict",
        "MATCH (a)-[:BLOCKS*]->(b) RETURN b",
        "E004 1 19",
        None,
    )
    .unwrap();
    // #88 is unknown in an empty store and known in a context that names it.
    err(
        "unknown-node",
        "MATCH (t {id: #88}) RETURN t",
        "E111 1 15",
        None,
    )
    .unwrap();
    let ctx = format!("schema core\nnode 88 {}\n", "0".repeat(31) + "8");
    assert!(
        err(
            "known-node",
            "MATCH (t {id: #88}) RETURN t",
            "E111 1 15",
            Some(&ctx)
        )
        .is_err()
    );
    err(
        "unbound-param",
        "MATCH (t) WHERE t.estimate > $n RETURN t",
        "E110 1 30",
        None,
    )
    .unwrap();
    let ctx = "schema core\nparam n int 3\n";
    assert!(
        err(
            "bound-param",
            "MATCH (t) WHERE t.estimate > $n RETURN t",
            "E110 1 30",
            Some(ctx)
        )
        .is_err()
    );
    assert!(Context::parse(&["schema other"]).is_err());
    assert!(case("nothing", "RETURN 1 AS x", Expect::default()).is_err());
}
