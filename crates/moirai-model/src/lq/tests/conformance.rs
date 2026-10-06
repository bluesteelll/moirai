//! The conformance fixtures of `fixtures/lq/` (WP-22, [50 §8.3]): the token, AST and error cases that WP-93a's
//! acceptance names, and the standard-library sources of `fixtures/lq/std/` ([LQ/std §1]).
//!
//! No chapter fixes the files' layout; WP-22 names `fixtures/lq/INDEX.md` §2 as its owner, and until that section
//! exists this runner reads the layout WP-22's files show. It reads two layouts, each carrying exactly the formats the
//! chapters define:
//!
//! - **Case files** `*.cases` anywhere under `fixtures/lq/` except `std/`, the layout WP-22 writes. Lines before the
//!   first case that start with `#` are comments. A case runs from `%% case <name>` to `%% end`.
//!   - One-line directives: `%% source …` and `%% note …`, not read; `%% entry read|write|define`; `%% mode
//!     strict-gql`; `%% input-file <path under fixtures/lq>`; `%% sast-same-as <case>` and `%% cast-same-as <case>`,
//!     the S-AST, or the C-AST encoding, of another case of any file; `%% accept`, the text parses; `%% error <code>
//!     <line>:<col> [spec|conv]`, `<code> *` for a position no chapter fixes, or `<code>` alone for an unlocated error
//!     (a decoding error, a parse error, else the first bind error); for the binder, `%% profile
//!     gated|compatible|unknown`, `%% display cypher|gql` (the echo's quantifier spelling), `%% context <line>`
//!     (`branch <ref>`; `rev <n>`, the tip the JSON error envelope names, not read; or a binding-context line of §9
//!     below), `%% outcome runs|error`, `%% warnings <codes>|none` and `%% notices <codes>|none` (the codes the binder
//!     decides); for the C-AST, `%% hash blake3_256 0..<n> first 16 = <32 hex>` (`H` of [LQ/canonical-ast §7.1] over
//!     the whole encoding), `%% explain-id q:<8 hex>` and `%% cursor-query-hash 0x<16 hex>` (§7.2).
//!   - Blocks run to the next directive, trailing empty lines dropped: `%% input`, the text, lines joined by LF;
//!     `%% input-json`, a JSON IR document in the same line form ([LQ/canonical-ast §9]: the directive, not the file
//!     name or the entry value, tells it from LQ text); `%% input-hex`, its bytes in lower-case hex (text from `;` to
//!     the end of a line is a comment); `%% tokens`, the
//!     token stream of [LQ/lexical §11], compared byte for byte; `%% sast`, the S-expression of [LQ/canonical-ast
//!     §4.2], compared by §4.1; `%% reads`, the reading-echo lines of [LQ/envelope §4.2], in order; `%% cast`, the
//!     C-AST's S-expression of §4.3 in the case's context; `%% encoding`, its bytes (§6) in hex; `%% portable`, the
//!     stored text of the definition (§8.1).
//!   - Renderings are read and not checked, since the model has no rendering and no JSON code (PLAN §2.2): `%% text`
//!     and `%% json` (the text and JSON forms of a result or an error: the reference renderer's, WP-71a, and the
//!     product's), `%% json-ir` (the JSON IR output form), `%% hex` (the bytes of an envelope structure such as a
//!     cursor) and `%% transport` (the source name of an error text). A case without an input states only renderings
//!     (`%% hash` included, over its `%% hex`) and is not run. A case holds at most one of the byte blocks `%% encoding`
//!     and `%% hex` ([LQ/canonical-ast §9] "Hash lines"). A case whose input is `%% input-json` is read and not run:
//!     the model has no JSON code, the IR reader being the converter's. The cases of `json-ir.cases` take their IR
//!     documents by `%% input-json`, and their errors may be located by a JSON Pointer (`<code> ptr <pointer>
//!     spec|conv`): that file's layout is read and its cases are not run.
//!   - A case binds in a store where every node its text names by `#N` exists, kinds unknown ("a store in which the
//!     named nodes exist"), besides the nodes its context names. Any other directive, a directive given twice, or text
//!     outside a block, fails the check: the layout has moved on.
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
//! `gt10/`). Until it holds a case the check has nothing to read; once any later directory exists, a missing
//! `fixtures/lq` fails the check, and so does a `fixtures/lq` without a case. A `fixtures/lq` that holds files outside
//! `std/` but no case fails it too: its layout is not the one this runner reads.

use crate::lq::cast::{Root, encode, sexpr as cast_sexpr};
use crate::lq::ctx::{BindCtx, Caller, Identities, MapIds, Params, Value};
use crate::lq::diag::{Diag, line_col};
use crate::lq::lexer::decode;
use crate::lq::parser::{ParseOptions, parse_define, parse_read, parse_write, token_stream_text};
use crate::lq::schema::Schema;
use crate::lq::{bind, sexpr};
use std::collections::BTreeMap;
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

/// Whether a first error as [`first_error`] writes it matches a fixture's: equal, or the same code when the fixture
/// writes `<code> *` (a position no chapter fixes).
fn error_matches(got: &str, want: &str) -> bool {
    match want.strip_suffix(" *") {
        Some(code) => got.split_whitespace().next() == Some(code),
        None => got == want,
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
    check_as(entry, opts, bytes, x)
}

/// Decodes a case's bytes ([LQ/lexical §2.1]). A refusal is the first error as [`first_error`] writes it: E003 at the
/// first byte that is not well-formed, its line and column counted over the well-formed prefix of the text after the
/// byte-order mark ([LQ/lexical §2.3]), which is the whole text before the offset.
fn decode_case(bytes: &[u8]) -> Result<&str, String> {
    decode(bytes).map_err(|d| {
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let at = d.span.map_or(0, |s| s.start);
        let prefix = body.get(..at as usize).unwrap_or(body);
        match std::str::from_utf8(prefix) {
            Ok(prefix) => {
                let (l, c) = line_col(prefix, at);
                format!("{} {l} {c}", d.code)
            }
            Err(_) => d.code.to_string(),
        }
    })
}

/// Checks one case's text with an entry and mode against its expectations.
fn check_as(entry: Entry, opts: ParseOptions, bytes: &[u8], x: &Expect) -> Result<(), String> {
    if x.tokens.is_none() && x.sast.is_none() && x.err.is_none() && x.cast.is_none() {
        return Err("no expectation file".into());
    }
    let src = match decode_case(bytes) {
        Ok(s) => s,
        Err(got) => {
            return match &x.err {
                Some(want) if x.tokens.is_none() && x.sast.is_none() && x.cast.is_none() => {
                    if error_matches(&got, want.trim()) {
                        Ok(())
                    } else {
                        Err(format!(
                            "first error {got}, the fixture expects {}",
                            want.trim()
                        ))
                    }
                }
                _ => Err(format!("does not decode: {got}")),
            };
        }
    };
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
        if !error_matches(&got, want) {
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

/// One case of a `.cases` file: its name, the line of its `%% case`, and its directives.
#[derive(Default)]
struct Block {
    name: String,
    line: usize,
    entry: Option<String>,
    strict: bool,
    profile: Option<String>,
    display: Option<String>,
    context: Vec<String>,
    input: Option<String>,
    input_file: Option<String>,
    input_hex: Option<String>,
    /// A JSON IR document ([LQ/json-ir]), which the model reads without running.
    input_json: Option<String>,
    tokens: Option<String>,
    sast: Option<String>,
    same_as: Option<String>,
    accept: bool,
    error: Option<String>,
    outcome: Option<String>,
    warnings: Option<String>,
    notices: Option<String>,
    reads: Option<String>,
    cast: Option<String>,
    cast_same_as: Option<String>,
    encoding: Option<String>,
    hash: Option<String>,
    explain_id: Option<String>,
    cursor_hash: Option<String>,
    portable: Option<String>,
    /// A rendering no model code produces was stated: `%% text`, `%% json`, `%% json-ir`, `%% hex` or `%% transport`.
    renders: bool,
    /// The byte block `%% hex` was stated (at most one of it and `%% encoding`, [LQ/canonical-ast §9]).
    hex: bool,
}

impl Block {
    /// Whether the case has a text for the front end.
    fn has_input(&self) -> bool {
        self.input.is_some() || self.input_file.is_some() || self.input_hex.is_some()
    }

    /// Whether the case states its C-AST or a value derived from its encoding ([LQ/canonical-ast §4.3], §6, §7, §8).
    fn casts(&self) -> bool {
        self.cast.is_some()
            || self.cast_same_as.is_some()
            || self.encoding.is_some()
            || self.hash.is_some()
            || self.explain_id.is_some()
            || self.cursor_hash.is_some()
            || self.portable.is_some()
    }

    /// Whether the case binds: it states what the binder decides or what it builds.
    fn binds(&self) -> bool {
        self.outcome.is_some()
            || self.warnings.is_some()
            || self.notices.is_some()
            || self.reads.is_some()
            || self.casts()
    }
}

/// What binding a case decided: its warnings and notices by code, its reading-echo lines, its C-AST (the S-expression
/// of [LQ/canonical-ast §4.3] and the encoding of §6) and the portable texts of the definitions it bound (§8.1).
struct Decided {
    warnings: Vec<String>,
    notices: Vec<String>,
    reads: Vec<String>,
    cast: String,
    encoding: Vec<u8>,
    portable: Vec<String>,
}

impl Decided {
    fn of<T>(x: &bind::Bound<T>, root: Root<'_>) -> Decided {
        let codes = |w: char| {
            x.lints
                .iter()
                .map(|l| l.code.as_str().to_string())
                .filter(|c| c.starts_with(w))
                .collect()
        };
        Decided {
            warnings: codes('W'),
            notices: codes('N'),
            reads: x.reads.clone(),
            cast: cast_sexpr(root),
            encoding: encode(root),
            portable: x.portable.iter().map(|(_, t)| t.clone()).collect(),
        }
    }
}

/// Binds a case's tree in its context ([`Block::context`]: `schema core`, `branch <ref>`, `rev <n>` and the lines of
/// [LQ/canonical-ast §9]) as a caller of its profile and display spelling, in a store where every node its text names by
/// `#N` exists (the case files state their store as "a store in which the named nodes exist"; kinds unknown). `rev <n>`
/// is the caller's tip for the JSON error envelope, a rendering, and binding does not read it.
fn bind_case(
    src: &str,
    tree: &Tree,
    b: &Block,
    tokens: &str,
) -> Result<Result<Decided, Vec<Diag>>, String> {
    let mut caller = Caller::default();
    let mut lines = Vec::new();
    for c in &b.context {
        match c.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["branch", r] => caller.branch = (*r).to_string(),
            ["rev", n] if n.parse::<u64>().is_ok() => {}
            _ => lines.push(c.as_str()),
        }
    }
    let mut cx = Context::parse(&lines)?;
    for n in tokens.lines().filter_map(|l| l.strip_prefix("NODE ")) {
        let n: u32 = n.parse().map_err(|_| format!("token NODE {n}"))?;
        if cx.ids.uid(n).is_none() {
            let mut u = [0u8; 16];
            u[..4].copy_from_slice(&n.to_be_bytes());
            u[15] = 0x4e;
            cx.ids.node(n, u, None);
        }
    }
    caller.profile = match b.profile.as_deref() {
        None | Some("compatible") => crate::lq::ctx::Profile::Compatible,
        Some("gated") => crate::lq::ctx::Profile::Gated,
        Some("unknown") => crate::lq::ctx::Profile::Unknown,
        Some(p) => return Err(format!("profile {p:?}")),
    };
    caller.display = match b.display.as_deref() {
        None | Some("cypher") => crate::lq::printer::Spelling::Cypher,
        Some("gql") => crate::lq::printer::Spelling::Gql,
        Some(d) => return Err(format!("display {d:?}")),
    };
    let ctx = BindCtx {
        schema: &cx.schema,
        ids: &cx.ids,
        params: &cx.params,
        caller: &caller,
    };
    Ok(match tree {
        Tree::Read(r) => {
            bind::bind_read(&ctx, src, r).map(|x| Decided::of(&x, Root::Query(&x.ast)))
        }
        Tree::Write(w) => bind::bind_write(&ctx, src, w).map(|x| Decided::of(&x, Root::Tx(&x.ast))),
        Tree::Define(d) => {
            bind::bind_define(&ctx, src, d).map(|x| Decided::of(&x, Root::Define(&x.ast)))
        }
    })
}

/// A `%% warnings` or `%% notices` line as a list of codes (`none` is the empty list).
fn code_list(s: &str) -> Vec<String> {
    s.split([' ', ','])
        .filter(|w| !w.is_empty() && *w != "none")
        .map(str::to_string)
        .collect()
}

/// The bytes of a hex block (`%% input-hex`, `%% encoding`): lower-case hex digits in groups of whole bytes, separated
/// by white space; text from `;` to the end of a line is a comment.
fn hex_block(text: &str) -> Result<Vec<u8>, String> {
    let digits: Vec<&str> = text
        .lines()
        .map(|l| l.split_once(';').map_or(l, |(h, _)| h))
        .collect();
    unhex_vec(&digits.join(" "))
}

/// Checks `%% hash blake3_256 0..<n> first 16 = <32 hex>`: `H` of [LQ/canonical-ast §7.1] over the whole encoding.
fn check_hash(spec: &str, encoding: &[u8]) -> Result<(), String> {
    let w: Vec<&str> = spec.split_whitespace().collect();
    let ["blake3_256", range, "first", "16", "=", want] = w.as_slice() else {
        return Err(format!("hash {spec:?}"));
    };
    let n: usize = range
        .strip_prefix("0..")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("hash range {range:?}"))?;
    if n != encoding.len() {
        return Err(format!(
            "the hash covers 0..{n}, the encoding has {} bytes",
            encoding.len()
        ));
    }
    let got = hex(&blake3::hash(encoding).as_bytes()[..16]);
    if got != *want {
        return Err(format!("H {got}, the fixture has {want}"));
    }
    Ok(())
}

/// The cases of a `.cases` file (see the module documentation), or what in it this runner cannot read.
fn read_blocks(text: &str) -> Result<Vec<Block>, String> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    // The directive whose lines are being collected, and the lines.
    let mut body: Option<(&str, Vec<&str>)> = None;
    let close = |cur: &mut Option<Block>, body: &mut Option<(&str, Vec<&str>)>| {
        let (Some(b), Some((k, mut lines))) = (cur.as_mut(), body.take()) else {
            return;
        };
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let joined = lines.join("\n");
        match k {
            "input" => b.input = Some(joined),
            "input-hex" => b.input_hex = Some(joined),
            "input-json" => b.input_json = Some(joined),
            "tokens" => b.tokens = Some(joined + "\n"),
            "sast" => b.sast = Some(joined),
            "reads" => b.reads = Some(joined),
            "cast" => b.cast = Some(joined),
            "encoding" => b.encoding = Some(joined),
            "portable" => b.portable = Some(joined),
            // The text and JSON renderings of a result or an error, the JSON IR output form and the bytes of an
            // envelope structure: the reference renderer's (WP-71a), the JSON IR converter's and the product's. The
            // model has no rendering and no JSON code (PLAN §2.2).
            "hex" => {
                b.hex = true;
                b.renders = true;
            }
            _ => b.renders = true,
        }
    };
    for (i, line) in text.lines().enumerate() {
        let Some(d) = line.strip_prefix("%% ") else {
            match &mut body {
                Some((_, lines)) => lines.push(line),
                None if line.trim().is_empty() || (cur.is_none() && line.starts_with('#')) => {}
                None => return Err(format!("line {}: text outside a case's blocks", i + 1)),
            }
            continue;
        };
        close(&mut cur, &mut body);
        let (k, rest) = d.split_once(' ').unwrap_or((d, ""));
        let rest = rest.trim();
        if k == "case" {
            if cur.is_some() {
                return Err(format!("line {}: a case without %% end", i + 1));
            }
            cur = Some(Block {
                name: rest.to_string(),
                line: i + 1,
                ..Block::default()
            });
            continue;
        }
        let Some(b) = cur.as_mut() else {
            return Err(format!("line {}: %% {k} outside a case", i + 1));
        };
        let one = |v: &Option<String>| match v {
            Some(_) => Err(format!("line {}: a second %% {k}", i + 1)),
            None => Ok(Some(rest.to_string())),
        };
        match k {
            "source" | "note" => {}
            "entry" => b.entry = one(&b.entry)?,
            "input-file" => b.input_file = one(&b.input_file)?,
            "sast-same-as" => b.same_as = one(&b.same_as)?,
            "cast-same-as" => b.cast_same_as = one(&b.cast_same_as)?,
            "error" => b.error = one(&b.error)?,
            "accept" if rest.is_empty() => b.accept = true,
            "mode" if rest == "strict-gql" => b.strict = true,
            "profile" => b.profile = one(&b.profile)?,
            "display" if matches!(rest, "cypher" | "gql") => b.display = one(&b.display)?,
            "context" => b.context.push(rest.to_string()),
            "outcome" if matches!(rest, "runs" | "error") => b.outcome = one(&b.outcome)?,
            "warnings" => b.warnings = one(&b.warnings)?,
            "notices" => b.notices = one(&b.notices)?,
            "hash" => b.hash = one(&b.hash)?,
            "explain-id" => b.explain_id = one(&b.explain_id)?,
            "cursor-query-hash" => b.cursor_hash = one(&b.cursor_hash)?,
            // The source name of an error's text rendering: `argv`, `stdin`, `query` or `file <name>`.
            "transport" => b.renders = true,
            "input" | "input-hex" | "input-json" | "tokens" | "sast" | "reads" | "cast"
            | "encoding" | "portable" | "json-ir" | "text" | "json" | "hex" => {
                let given = match k {
                    "input" => b.input.is_some(),
                    "input-hex" => b.input_hex.is_some(),
                    "input-json" => b.input_json.is_some(),
                    "encoding" => b.encoding.is_some(),
                    "hex" => b.hex,
                    _ => false,
                };
                if given {
                    return Err(format!("line {}: a second %% {k}", i + 1));
                }
                body = Some((k, Vec::new()));
            }
            "end" => {
                if let Some(b) = &cur
                    && b.encoding.is_some()
                    && b.hex
                {
                    return Err(format!(
                        "case {}: both %% encoding and %% hex ([LQ/canonical-ast §9])",
                        b.name
                    ));
                }
                out.extend(cur.take());
            }
            _ => return Err(format!("line {}: unknown directive %% {k}", i + 1)),
        }
    }
    close(&mut cur, &mut body);
    match cur {
        Some(b) => Err(format!("case {} has no %% end", b.name)),
        None => Ok(out),
    }
}

/// What checking one case left for the checks across cases: its S-AST when its text parsed, and its C-AST encoding
/// when it bound.
#[derive(Default)]
struct Ran {
    sast: Option<String>,
    encoding: Option<Vec<u8>>,
}

/// The first error a case expects: `<code> <line>:<col> [spec|conv]` as [`first_error`] writes it, `<code> *` for an
/// error whose position no chapter fixes, or `<code>` for an unlocated one.
fn expected_error(e: &str) -> Result<String, String> {
    let w: Vec<&str> = e.split_whitespace().collect();
    let at = match w.get(1).map(|p| p.split_once(':')) {
        None => String::new(),
        Some(None) if w[1] == "*" => " *".to_string(),
        Some(Some((l, c))) if l.parse::<u32>().is_ok() && c.parse::<u32>().is_ok() => {
            format!(" {l} {c}")
        }
        Some(_) => return Err(format!("error {e:?}")),
    };
    if w.len() > 3 || w.get(2).is_some_and(|c| !["spec", "conv"].contains(c)) {
        return Err(format!("error {e:?}"));
    }
    Ok(format!("{}{at}", w.first().copied().unwrap_or("")))
}

/// Checks one case with an input against its expectations. `lq` is `fixtures/lq`, against which `%% input-file`
/// resolves.
fn run_block(lq: &Path, b: &Block) -> Result<Ran, String> {
    let mut words = b.entry.as_deref().unwrap_or("").split_whitespace();
    let entry = match words.next() {
        Some("read") => Entry::Read,
        Some("write") => Entry::Write,
        Some("define") => Entry::Define,
        other => return Err(format!("entry {other:?}")),
    };
    let mut opts = ParseOptions {
        strict_gql: b.strict,
    };
    for w in words {
        match w {
            "strict" | "strict-gql" => opts.strict_gql = true,
            _ => return Err(format!("entry word {w:?}")),
        }
    }
    let bytes = match (&b.input, &b.input_file, &b.input_hex) {
        (Some(t), None, None) => t.clone().into_bytes(),
        (None, Some(f), None) => std::fs::read(lq.join(f)).map_err(|e| format!("{f}: {e}"))?,
        (None, None, Some(h)) => hex_block(h)?,
        _ => return Err("one of %% input, %% input-file and %% input-hex".into()),
    };
    let err = b.error.as_deref().map(expected_error).transpose()?;
    if b.outcome.as_deref() == Some("error") && err.is_none() {
        return Err("%% outcome error without %% error".into());
    }
    if err.is_some() && (b.accept || b.outcome.as_deref() == Some("runs") || b.casts()) {
        return Err("%% error with an expectation of success".into());
    }
    if b.tokens.is_none()
        && b.sast.is_none()
        && err.is_none()
        && !b.accept
        && !b.binds()
        && b.same_as.is_none()
    {
        return Err("no expectation".into());
    }
    // An expected error stops the case: compare it.
    let stop = |got: String, what: &str| match &err {
        Some(want) if error_matches(&got, want) => Ok(Ran::default()),
        Some(want) => Err(format!("first error {got}, the fixture expects {want}")),
        None => Err(format!("{what}: {got}")),
    };
    let src = match decode_case(&bytes) {
        Ok(s) => s,
        Err(got) => return stop(got, "does not decode"),
    };
    let (tree, tokens) = match parse(src, entry, opts) {
        Ok(t) => t,
        Err(e) => return stop(first_error(src, &e), "does not parse"),
    };
    if let Some(want) = &b.tokens
        && tokens != *want
    {
        return Err(format!("token stream:\n{tokens}the fixture has:\n{want}"));
    }
    let s = sast(&tree);
    if let Some(want) = &b.sast
        && !sexpr::same(&s, want)
    {
        return Err(format!("S-AST {s}, the fixture has {}", want.trim()));
    }
    let mut ran = Ran {
        sast: Some(s),
        encoding: None,
    };
    if err.is_none() && !b.binds() {
        return Ok(ran);
    }
    let d = match bind_case(src, &tree, b, &tokens)? {
        Err(e) => {
            let got = first_error(src, &e);
            stop(got, "does not bind")?;
            return Ok(ran);
        }
        Ok(d) => d,
    };
    if let Some(want) = &err {
        return Err(format!("parses and binds; the fixture expects {want}"));
    }
    if let Some(w) = &b.warnings
        && d.warnings != code_list(w)
    {
        return Err(format!("warnings {:?}, the fixture has {w}", d.warnings));
    }
    if let Some(n) = &b.notices
        && d.notices != code_list(n)
    {
        return Err(format!("notices {:?}, the fixture has {n}", d.notices));
    }
    if let Some(r) = &b.reads {
        let want: Vec<&str> = r.lines().filter(|l| !l.trim().is_empty()).collect();
        let got: Vec<String> = d.reads.iter().map(|l| format!("reads: {l}")).collect();
        if got != want {
            return Err(format!("reads {got:?}, the fixture has {want:?}"));
        }
    }
    if let Some(want) = &b.cast
        && !sexpr::same(&d.cast, want)
    {
        return Err(format!("C-AST {}, the fixture has {}", d.cast, want.trim()));
    }
    if let Some(want) = &b.encoding
        && d.encoding != hex_block(want)?
    {
        return Err(format!(
            "encoding {}, the fixture has {want}",
            hex(&d.encoding)
        ));
    }
    let h = blake3::hash(&d.encoding);
    let h = h.as_bytes();
    if let Some(spec) = &b.hash {
        check_hash(spec, &d.encoding)?;
    }
    if let Some(want) = &b.explain_id {
        let got = format!("q:{}", hex(&h[..4]));
        if got != *want {
            return Err(format!("EXPLAIN id {got}, the fixture has {want}"));
        }
    }
    if let Some(want) = &b.cursor_hash {
        let mut q = [0u8; 8];
        q.copy_from_slice(&h[..8]);
        let got = format!("0x{:016x}", u64::from_le_bytes(q));
        if got != *want {
            return Err(format!("cursor query hash {got}, the fixture has {want}"));
        }
    }
    if let Some(want) = &b.portable
        && d.portable.as_slice() != std::slice::from_ref(want)
    {
        return Err(format!(
            "portable texts {:?}, the fixture has {want:?}",
            d.portable
        ));
    }
    // `%% outcome runs`: the input evaluates without error on the fixture store of INDEX.md §2.3 (WP-93b).
    if b.outcome.as_deref() == Some("runs") {
        crate::lq::eval::tests::runs(src, entry == Entry::Write)?;
    }
    ran.encoding = Some(d.encoding);
    Ok(ran)
}

/// How many cases a run checked, and how many it read without running: cases that state only a rendering (no input),
/// and the cases of the JSON IR's file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    checked: usize,
    renderings: usize,
    json_ir: usize,
}

/// What the checks across cases need: each case's S-AST and C-AST encoding by name, and each `%% sast-same-as` and
/// `%% cast-same-as` as (file, case, other case).
#[derive(Default)]
struct Across {
    sasts: BTreeMap<String, String>,
    encodings: BTreeMap<String, Vec<u8>>,
    same_sast: Vec<(String, String, String)>,
    same_cast: Vec<(String, String, String)>,
}

/// The file of the JSON IR's cases ([LQ/json-ir]): its inputs are IR documents, which the model has no code for (PLAN
/// §2.2; the reader is the converter's). Its layout is read and its cases are not run.
const JSON_IR_FILE: &str = "json-ir.cases";

/// Checks the cases of one `.cases` file into `tally`; failures go to `failures`, and what the checks across cases need
/// to `across`. `lq` is `fixtures/lq`.
fn run_block_file(
    lq: &Path,
    p: &Path,
    tally: &mut Tally,
    failures: &mut Vec<String>,
    across: &mut Across,
) {
    let text = match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| {
        decode(&b)
            .map(str::to_string)
            .map_err(|d| format!("{} (decoding)", d.code))
    }) {
        Ok(t) => t,
        Err(e) => {
            failures.push(format!("{}: {e}", p.display()));
            return;
        }
    };
    let blocks = match read_blocks(&text) {
        Ok(b) => b,
        Err(e) => {
            failures.push(format!("{}: {e}", p.display()));
            return;
        }
    };
    let ir_file = p.file_name().is_some_and(|n| n == JSON_IR_FILE);
    for b in &blocks {
        let at = format!("{}: case {} (line {})", p.display(), b.name, b.line);
        if b.input_json.is_some() || ir_file {
            // A JSON IR document: read, not run (the model has no JSON code, PLAN §2.2). The JSON IR's file takes its
            // documents by `%% input-json`, and no case holds both an IR document and an LQ input ([LQ/canonical-ast
            // §9]).
            if b.has_input() {
                failures.push(format!(
                    "{at}: a JSON IR case takes its document by %% input-json alone"
                ));
            } else {
                tally.json_ir += 1;
            }
            continue;
        }
        if !b.has_input() {
            // A rendering golden: a result, an error text or envelope bytes for a stated situation.
            let only_renders = b.renders
                && b.entry.is_none()
                && b.tokens.is_none()
                && b.sast.is_none()
                && b.same_as.is_none()
                && b.error.is_none()
                && !b.accept
                && b.outcome.is_none()
                && b.warnings.is_none()
                && b.notices.is_none()
                && b.reads.is_none()
                && b.cast.is_none()
                && b.cast_same_as.is_none()
                && b.encoding.is_none()
                && b.explain_id.is_none()
                && b.cursor_hash.is_none()
                && b.portable.is_none();
            if only_renders {
                tally.renderings += 1;
            } else {
                failures.push(format!("{at}: no %% input, %% input-file or %% input-hex"));
            }
            continue;
        }
        tally.checked += 1;
        match run_block(lq, b) {
            Ok(ran) => {
                if let Some(s) = ran.sast {
                    across.sasts.insert(b.name.clone(), s);
                }
                if let Some(e) = ran.encoding {
                    across.encodings.insert(b.name.clone(), e);
                }
            }
            Err(e) => failures.push(format!("{at}: {e}")),
        }
        let file = p.display().to_string();
        if let Some(other) = &b.same_as {
            across
                .same_sast
                .push((file.clone(), b.name.clone(), other.clone()));
        }
        if let Some(other) = &b.cast_same_as {
            across.same_cast.push((file, b.name.clone(), other.clone()));
        }
    }
}

/// Runs every case under `lq` (not `lq/std/`): the tally, or the failures, each `path: why`.
fn run_cases(lq: &Path) -> Result<Tally, Vec<String>> {
    let mut all = Vec::new();
    files(lq, &mut all).map_err(|e| vec![format!("{}: {e}", lq.display())])?;
    let std_dir = lq.join("std");
    let mut tally = Tally::default();
    let mut failures = Vec::new();
    let mut across = Across::default();
    for p in all
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "cases") && !p.starts_with(&std_dir))
    {
        run_block_file(lq, p, &mut tally, &mut failures, &mut across);
    }
    for (file, name, other) in &across.same_sast {
        match (across.sasts.get(name), across.sasts.get(other)) {
            (Some(x), Some(y)) if sexpr::same(x, y) => {}
            (x, y) => failures.push(format!(
                "{file}: case {name}: S-AST {x:?}, the S-AST of case {other} is {y:?}"
            )),
        }
    }
    for (file, name, other) in &across.same_cast {
        match (across.encodings.get(name), across.encodings.get(other)) {
            (Some(x), Some(y)) if x == y => {}
            (x, y) => failures.push(format!(
                "{file}: case {name}: C-AST encoding {:?}, the encoding of case {other} is {:?}",
                x.map(|x| hex(x)),
                y.map(|y| hex(y))
            )),
        }
    }
    for p in all
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "lq") && !p.starts_with(&std_dir))
    {
        tally.checked += 1;
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
        Ok(tally)
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
        Ok(t) if t.checked == 0 => {
            // Only `std/` so far is WP-22 at work. A file outside it that no case claims is a layout this runner does
            // not read, and a later directory means the cases should be there.
            let mut all = Vec::new();
            files(&lq, &mut all).unwrap_or_else(|e| panic!("{}: {e}", lq.display()));
            let std_dir = lq.join("std");
            let unread: Vec<String> = all
                .iter()
                .filter(|p| !p.starts_with(&std_dir))
                .map(|p| p.display().to_string())
                .collect();
            let later: Vec<&str> = LATER
                .iter()
                .copied()
                .filter(|d| root.join(d).is_dir())
                .collect();
            assert!(
                unread.is_empty() && later.is_empty(),
                "fixtures/lq holds no case (files outside std/: {unread:?}; later directories: {later:?})"
            );
        }
        Ok(t) => println!(
            "{} cases checked; {} renderings and {} JSON IR cases read, not run",
            t.checked, t.renderings, t.json_ir
        ),
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
        assert_eq!(run_cases(&dir).map(|t| t.checked), Ok(3));
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

/// The runner over case files: every directive it reads, a failing case reported with its name, and a directive it
/// does not know failing the file.
#[test]
fn the_runner_reads_case_files() {
    let dir = std::env::temp_dir().join(format!("moirai-model-lq-cases-{}", std::process::id()));
    let result = std::panic::catch_unwind(|| {
        let write = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write("std/one.lq", "DEFINE QUERY one() AS { RETURN 1 AS x }\n");
        write(
            "cases/a.cases",
            "# a comment before the first case\n\n\
             %% case tokens\n%% source [LQ/lexical §11]\n%% entry read\n%% input\nRETURN 1 AS x\n\
             %% tokens\nKW RETURN\nINT 1\nKW AS\nNAME x\nEOF -\n\n%% end\n\n\
             %% case literal\n%% entry read\n%% input\nMATCH (#51) RETURN 1 AS x\n\
             %% sast\n(read run (query [(part _ [(match false _ [(path (npat _ [] [(kv \"id\" (nid 51))] _) [])] _)]\n \
             (return false false [(item (int 1) \"x\")] [] [] _) _)] []))\n%% end\n\
             %% case map\n%% entry read\n%% note one S-AST\n%% input\nMATCH ({id: #51}) RETURN 1 AS x\n\
             %% sast-same-as literal\n%% end\n\
             %% case strict\n%% entry read\n%% mode strict-gql\n%% input\nRETURN 1 != 2 AS x\n\
             %% error E004 1:10 spec\n%% end\n\
             %% case file\n%% entry define\n%% input-file std/one.lq\n%% tokens\nKW DEFINE\nKW QUERY\n\
             NAME one\nP (\nP )\nKW AS\nP {\nKW RETURN\nINT 1\nKW AS\nNAME x\nP }\nEOF -\n%% end\n\
             %% case echo\n%% entry read\n%% profile gated\n%% context schema core\n%% context branch main\n\
             %% input\nMATCH (#51)-[:BLOCKED_BY]->(b) RETURN b\n%% warnings none\n%% notices none\n\
             %% outcome runs\n%% reads\n\
             reads: b BLOCKS #51 (written #51 BLOCKED_BY b) | b must finish before #51 starts\n%% end\n\
             %% case bind-error\n%% entry read\n%% input\nMATCH (a:task)-[:DEPENDS_ON]->(b:task) RETURN a\n\
             %% outcome error\n%% error E106 *\n%% end\n\
             %% case json\n%% entry read\n%% input\nRETURN 1 AS x\n%% json-ir\n{\"t\":\"read\"}\n\
             %% tokens\nKW RETURN\nINT 1\nKW AS\nNAME x\nEOF -\n%% end\n",
        );
        assert_eq!(run_cases(&dir).map(|t| t.checked), Ok(8));
        // Input bytes in hex, decoding errors located over the well-formed prefix, success, C-AST values, renderings
        // and the JSON IR's file.
        let enc = "10 00 00 00 6d 6f 69 72 61 69 2d 6c 71 2d 61 73 74 2d 76 31 01 00 01 02 00 03 01 00 00 00 10 00
01 00 00 00 20 21 01 00 00 00 00 01 00 00 00 04 00 00 00 74 61 73 6b 00 00 00 00 00 00 00 00 00
01 31 33 01 3a 3b 00 00 00 00 06 00 00 00 73 74 61 74 75 73 58 04 00 00 00 6f 70 65 6e 33 04 3a
3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 52 01 00 00 00 00 00 00 00 14 00 00 01 00 00
00 16 3b 00 00 00 00 00 01 00 00 00 18 3a 3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 00
01 52 05 00 00 00 00 00 00 00 00 00 00 00";
        write(
            "cases/c.cases",
            &format!(
                "%% case bom\n%% entry read\n%% input-hex\n; EF BB BF, then RETURN 1\nef bb bf 52 45 54 55 52 4e \
                 20 31 ; RETURN 1\n%% tokens\nKW RETURN\nINT 1\nEOF -\n%% end\n\
                 %% case bad-byte\n%% entry read\n%% input-hex\n52 45 54 55 52 4e 20 27 61 ff 27\n\
                 %% error E003 1:10 spec\n%% end\n\
                 %% case bad-byte-line-2\n%% entry read\n%% input-hex\nef bb bf 52 45 54 55 52 4e 0a 27 d0 b6 ff 27\n\
                 %% error E003 2:3 spec\n%% end\n\
                 %% case accepted\n%% entry read\n%% input\nRETURN ((1)) AS x\n%% accept\n%% end\n\
                 %% case s44\n%% entry read\n%% context schema core\n%% input\n\
                 MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5\n\
                 %% cast\n(QUERY (PART _ (CLAUSES [(MATCH false [(PATH (NODEP 0 [\"task\"] [] _) [])]\n \
                 (AND (CMP = (PROP (VAR 0) \"status\") (ENUM \"open\")) (CMP <= (PROP (VAR 0) \"priority\") (INT 1))))]\n \
                 (RETURN false false [(RITEM (VAR 0) _)] [(SORT (PROP (VAR 0) \"priority\") false)] (INT 5)))) [])\n\
                 %% encoding\n{enc}\n%% hash blake3_256 0..174 first 16 = 073f2b4d1d0af442d00ad286347b22fb\n\
                 %% explain-id q:073f2b4d\n%% cursor-query-hash 0x42f40a1d4d2b3f07\n%% end\n\
                 %% case s44-lower\n%% entry read\n%% context schema core\n%% input\n\
                 match (x:Task) where x.status = open and x.priority <= 'P1' return x order by x.priority limit 5\n\
                 %% cast-same-as s44\n%% end\n\
                 %% case portable\n%% entry define\n%% context schema core\n\
                 %% context node 51 018f3c2e7a117b3c9d5e4c2f1a0b9e51\n%% input\n\
                 DEFINE QUERY q() AS {{ MATCH (t {{id: #51}}) RETURN t }}  \n\
                 %% portable\nDEFINE QUERY q() AS {{ MATCH (t {{id: #u:018f3c2e7a117b3c9d5e4c2f1a0b9e51}}) RETURN t }}\n\
                 %% end\n\
                 %% case error-text\n%% entry read\n%% transport argv\n%% context branch main\n%% context rev 4480\n\
                 %% input\nMATCH (t) RETURN t;\n%% error E005 1:19 spec\n%% text\nerror[E005 one_statement]: …\n\
                 %% json\n{{\"v\":1}}\n%% end\n\
                 %% case rendering\n%% note a result for a stated situation\n%% text\nbranch: main | rev 4480 | 0 rows\n\
                 %% json\n{{\"v\":1}}\n%% end\n\
                 %% case digest\n%% hash blake3_256 0..1 first 16 = 00000000000000000000000000000000\n%% hex\n\
                 00 ; one byte\n%% end\n"
            ),
        );
        write(
            "cases/json-ir.cases",
            "%% case ir\n%% entry read\n%% input-json\n{\"t\": \"read\"}\n%% error E001 ptr /query spec\n%% end\n",
        );
        assert_eq!(
            run_cases(&dir),
            Ok(Tally {
                checked: 16,
                renderings: 2,
                json_ir: 1
            })
        );
        // A wrong C-AST value fails its case; so does a case with no input and no rendering.
        write(
            "cases/d.cases",
            "%% case wrong-id\n%% entry read\n%% context schema core\n%% input\n\
             MATCH (t:task) WHERE t.status = 'open' AND t.priority <= 1 RETURN t ORDER BY t.priority LIMIT 5\n\
             %% explain-id q:00000000\n%% end\n\
             %% case no-input\n%% tokens\nEOF -\n%% end\n",
        );
        let failures = run_cases(&dir).unwrap_err();
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert!(
            failures[0].contains("EXPLAIN id q:073f2b4d"),
            "{failures:?}"
        );
        assert!(failures[1].contains("case no-input"), "{failures:?}");
        let _ = std::fs::remove_file(dir.join("cases/d.cases"));
        write(
            "cases/b.cases",
            "%% case wrong\n%% entry read\n%% input\nRETURN 1 AS x\n%% error E001 1:1 spec\n%% end\n",
        );
        let failures = run_cases(&dir).unwrap_err();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("case wrong"), "{failures:?}");
        write(
            "cases/b.cases",
            "%% case new\n%% entry read\n%% future thing\n%% input\nRETURN 1 AS x\n%% end\n",
        );
        let failures = run_cases(&dir).unwrap_err();
        assert!(
            failures[0].contains("unknown directive %% future"),
            "{failures:?}"
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// [LQ/std §1], §2.6: every `fixtures/lq/std/<name>.lq` is the source of `std.<name>`: the chapter's block byte for byte
/// with one final LF, which parses with start symbol `define_stmt`, names `<name>`, and binds to the C-AST of the model's
/// own standard library.
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
            let raw = std::fs::read(p).map_err(|e| e.to_string())?;
            let src = decode(&raw).map_err(|d| format!("{} (decoding)", d.code))?;
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
            let block = crate::lq::catalog::STD_TEXTS
                .iter()
                .find(|t| {
                    t.strip_prefix("DEFINE QUERY ")
                        .and_then(|r| r.split('(').next())
                        == Some(name.as_str())
                })
                .ok_or_else(|| "the model has no text for it".to_string())?;
            if raw.strip_suffix(b"\n") != Some(block.as_bytes()) {
                return Err("is not the chapter's block byte for byte with one final LF".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            failures.push(format!("{}: {e}", p.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// One row of `fixtures/lq/std/catalog.txt` (its header): name, shape, budget class, cursor class and section, the
/// columns separated by blanks and the section (`[LQ/std §N.M]`, which holds a blank) last.
#[derive(Clone, Debug, PartialEq)]
struct CatalogRow {
    line: usize,
    name: String,
    shape: String,
    budget: String,
    cursor: String,
    section: String,
}

/// The rows of `catalog.txt`; `#` starts a comment, and a blank or comment line is no row.
fn catalog_rows(text: &str) -> Result<Vec<CatalogRow>, String> {
    let mut rows = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let l = raw.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        let mut cols = l.split_whitespace();
        let mut col = || cols.next().map(str::to_string);
        let (Some(name), Some(shape), Some(budget), Some(cursor)) = (col(), col(), col(), col())
        else {
            return Err(format!("line {}: fewer than five columns", i + 1));
        };
        let section = cols.collect::<Vec<_>>().join(" ");
        if section.is_empty() {
            return Err(format!("line {}: fewer than five columns", i + 1));
        }
        rows.push(CatalogRow {
            line: i + 1,
            name,
            shape,
            budget,
            cursor,
            section,
        });
    }
    Ok(rows)
}

/// `N.M` when the line opens the numbered paragraph `N.M.` of a chapter.
fn paragraph_number(l: &str) -> Option<&str> {
    let (num, _) = l.split_once(". ")?;
    let (a, b) = num.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    (digits(a) && digits(b)).then_some(num)
}

/// The name each `lq-define` block of [LQ/std] defines and the numbered paragraph it stands in (`4.22`), in the
/// chapter's order.
fn chapter_sections(md: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut section = "";
    let mut lines = md.lines();
    while let Some(l) = lines.next() {
        if l == "```lq-define" {
            let name = lines
                .next()
                .and_then(|d| d.strip_prefix("DEFINE QUERY "))
                .and_then(|d| d.split('(').next())
                .unwrap_or("");
            out.push((name.to_string(), section.to_string()));
            for b in lines.by_ref() {
                if b == "```" {
                    break;
                }
            }
        } else if let Some(n) = paragraph_number(l) {
            section = n;
        }
    }
    out
}

/// Checks `catalog.txt` and the `<name>.lq` files of `fixtures/lq/std/` against the model's standard library and the
/// chapter `md`: one row per definition in the chapter's order ([LQ/std §2.6]), its shape and budget class the bound
/// definition's (§2.3, §2.4: `table` and `medium` without the clause), its cursor class the bound one for a query of
/// the read catalog (§2.5, §3) and `-` for the pack and brief classes (§5, §6), its section the paragraph its block
/// stands in; and one file per row, no more (§2.6). Every mismatch, each `what: why`.
fn check_std_catalog(catalog: &str, md: &str, lq_files: &[String]) -> Vec<String> {
    let rows = match catalog_rows(catalog) {
        Ok(r) => r,
        Err(e) => return vec![format!("catalog.txt: {e}")],
    };
    let lib = crate::lq::catalog::std_catalog();
    let sections = chapter_sections(md);
    let mut failures = Vec::new();
    let ours: Vec<&str> = lib
        .iter()
        .map(|q| q.qname.strip_prefix("std.").unwrap_or(&q.qname))
        .collect();
    for name in &ours {
        if !rows.iter().any(|r| r.name == *name) {
            failures.push(format!("catalog.txt: no row for std.{name}"));
        }
        if !lq_files.iter().any(|f| f == name) {
            failures.push(format!("std/{name}.lq: missing"));
        }
    }
    for f in lq_files {
        if !ours.contains(&f.as_str()) {
            failures.push(format!("std/{f}.lq: the model has no std.{f}"));
        }
    }
    let placed = rows.iter().filter(|r| ours.contains(&r.name.as_str()));
    if let Some((r, want)) = ours
        .iter()
        .filter(|n| rows.iter().any(|r| r.name == **n))
        .zip(placed)
        .find_map(|(n, r)| (r.name != *n).then_some((r, *n)))
    {
        failures.push(format!(
            "catalog.txt line {}: {} out of the chapter's order (std.{want} comes here)",
            r.line, r.name
        ));
    }
    for (i, r) in rows.iter().enumerate() {
        let at = format!("catalog.txt line {} ({})", r.line, r.name);
        if rows[..i].iter().any(|p| p.name == r.name) {
            failures.push(format!("{at}: a second row"));
            continue;
        }
        let Some(q) = lib
            .iter()
            .find(|q| q.qname.strip_prefix("std.") == Some(&r.name))
        else {
            failures.push(format!("{at}: the model has no std.{}", r.name));
            continue;
        };
        let shape = q.cast.shape.as_deref().unwrap_or("table");
        if r.shape != shape {
            failures.push(format!("{at}: shape {}, the definition's {shape}", r.shape));
        }
        let budget = q.cast.budget.as_deref().unwrap_or("medium");
        if r.budget != budget {
            failures.push(format!(
                "{at}: budget {}, the definition's {budget}",
                r.budget
            ));
        }
        let Some((_, sec)) = sections.iter().find(|(n, _)| *n == r.name) else {
            failures.push(format!("{at}: the chapter has no lq-define block for it"));
            continue;
        };
        let section = format!("[LQ/std §{sec}]");
        if r.section != section {
            failures.push(format!(
                "{at}: section {}, the block stands in {section}",
                r.section
            ));
        }
        let cursor = if sec.starts_with("4.") {
            if q.live { "live" } else { "pinned" }
        } else {
            "-"
        };
        if r.cursor != cursor {
            failures.push(format!("{at}: cursor {}, expected {cursor}", r.cursor));
        }
    }
    failures
}

/// The stems of the `.lq` files directly in `dir`, in name order.
fn lq_stems(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "lq") {
            out.push(
                p.file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    out.sort();
    Ok(out)
}

/// The chapter [LQ/std], read at test time.
fn std_chapter() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/lq/std.md");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// [LQ/std §2.3]–§2.6, §9: `fixtures/lq/std/catalog.txt` and the files beside it describe the model's standard library.
#[test]
fn the_catalog_of_fixtures_lq_std() {
    let dir = fixtures().join("lq/std");
    if !dir.is_dir() {
        return;
    }
    let p = dir.join("catalog.txt");
    let catalog = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let stems = lq_stems(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let failures = check_std_catalog(&catalog, &std_chapter(), &stems);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The catalog the model and the chapter imply, in `catalog.txt`'s form, with every `.lq` file it needs.
fn implied_catalog(md: &str) -> (Vec<CatalogRow>, Vec<String>) {
    let sections = chapter_sections(md);
    let rows: Vec<CatalogRow> = crate::lq::catalog::std_catalog()
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let name = q.qname.strip_prefix("std.").unwrap_or(&q.qname).to_string();
            let sec = sections
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            let cursor = match (sec.starts_with("4."), q.live) {
                (true, true) => "live",
                (true, false) => "pinned",
                (false, _) => "-",
            };
            CatalogRow {
                line: i + 1,
                shape: q.cast.shape.clone().unwrap_or_else(|| "table".into()),
                budget: q.cast.budget.clone().unwrap_or_else(|| "medium".into()),
                cursor: cursor.into(),
                section: format!("[LQ/std §{sec}]"),
                name,
            }
        })
        .collect();
    let files = rows.iter().map(|r| r.name.clone()).collect();
    (rows, files)
}

/// `catalog.txt`'s text of `rows`.
fn catalog_text(rows: &[CatalogRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "{:<30} {:<10} {:<7} {:<7} {}\n",
                r.name, r.shape, r.budget, r.cursor, r.section
            )
        })
        .collect()
}

/// The catalog check accepts the catalog the model and the chapter imply, with comments and blank lines, and reports
/// every single defect of a row, a missing or extra file, a missing row and two rows out of order.
#[test]
fn the_catalog_check_reports_each_defect() {
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;
    let md = std_chapter();
    let (rows, files) = implied_catalog(&md);
    assert_eq!(rows.len(), 45);
    let text = format!("# a comment\n\n{}", catalog_text(&rows));
    assert_eq!(check_std_catalog(&text, &md, &files), Vec::<String>::new());
    for short in ["a b c\n", "# x\na b c d # e f\n"] {
        assert!(
            catalog_rows(short).is_err_and(|e| e.ends_with(": fewer than five columns")),
            "{short:?}"
        );
    }
    assert_eq!(
        catalog_rows("ready node light live [LQ/std §4.1] # c\n").map(|r| r[0].section.clone()),
        Ok("[LQ/std §4.1]".to_string())
    );
    let n = rows.len();
    let mut runner = super::runner(200);
    let check = |(i, j, kind): (usize, usize, u8)| -> Result<(), TestCaseError> {
        let mut rows = rows.clone();
        let mut files = files.clone();
        let name = rows[i].name.clone();
        let want = match kind {
            0 => {
                rows[i].shape.push('x');
                format!("({name}): shape")
            }
            1 => {
                rows[i].budget = if rows[i].budget == "heavy" {
                    "light"
                } else {
                    "heavy"
                }
                .into();
                format!("({name}): budget")
            }
            2 => {
                rows[i].cursor = match rows[i].cursor.as_str() {
                    "live" => "pinned",
                    "pinned" => "-",
                    _ => "live",
                }
                .into();
                format!("({name}): cursor")
            }
            3 => {
                rows[i].section = "[LQ/std §9.9]".into();
                format!("({name}): section")
            }
            4 => {
                files.retain(|f| *f != name);
                format!("std/{name}.lq: missing")
            }
            5 => {
                files.push("not_a_query".into());
                "std/not_a_query.lq: the model has no std.not_a_query".into()
            }
            6 => {
                rows.remove(i);
                format!("catalog.txt: no row for std.{name}")
            }
            _ => {
                let j = if j == i { (i + 1) % n } else { j };
                rows.swap(i, j);
                "out of the chapter's order".into()
            }
        };
        let got = check_std_catalog(&catalog_text(&rows), &md, &files);
        prop_assert!(got.iter().any(|g| g.contains(&want)), "{want}: {got:?}");
        prop_assert_eq!(got.len(), 1, "{:?}", got);
        Ok(())
    };
    if let Err(e) = runner.run(&(0..n, 0..n, 0u8..8), check) {
        panic!("{e}");
    }
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
        "E118 1 21",
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
