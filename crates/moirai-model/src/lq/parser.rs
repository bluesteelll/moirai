//! The LQ parser: grammar v1 ([LQ/grammar-v1.ebnf] §1–§6) by recursive descent, with the parser decisions of Annex P,
//! the refused forms of Annex R and the strict-GQL spelling mode of Annex G.
//!
//! The parser drives the [`Lexer`]: it reads normal-mode tokens through a small lookahead buffer and switches to
//! revision mode exactly at the revision positions of [LQ/lexical §4.2] (§P.2). It builds the S-AST of
//! [LQ/canonical-ast §3] with the spelling normalisations of §3.1 (§P.15), records the classified token stream of
//! [LQ/lexical §11] as it consumes tokens, bounds nesting at 64 levels (§P.13), and recovers at clause keywords, `;`
//! and `}` so that at most three errors are reported per pass (§P.14).

use crate::lq::ast::*;
use crate::lq::diag::{Code, Diag, Span, q};
use crate::lq::lexer::{Lexer, Punct, RevRead, TokKind, TokOut, Token, decode, json_string};
use crate::lq::printer;
use std::borrow::Cow;
use std::collections::VecDeque;

/// Parser options.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    /// The strict-GQL spelling mode ([LQ/grammar-v1.ebnf §G], [LQ/gql-spelling §3]): each Cypher-only spelling is E004.
    pub strict_gql: bool,
}

/// A successful parse: the tree and its classified token stream ([LQ/lexical §11]).
#[derive(Clone, Debug)]
pub struct Parsed<T> {
    /// The S-AST.
    pub tree: T,
    /// The classified tokens, ending with `EOF -`.
    pub tokens: Vec<TokOut>,
}

/// At most this many errors per pass ([LQ/grammar-v1.ebnf §P.14]).
pub const MAX_ERRORS: usize = 3;

/// The text of the E001 for the 65th nested entry ([LQ/grammar-v1.ebnf §P.13]).
const TOO_DEEP: &str = "nesting deeper than 64 levels";

/// The nesting limit of expressions and patterns ([LQ/grammar-v1.ebnf §P.13]).
pub const MAX_DEPTH: u32 = 64;

/// The stack of the front end's own thread ([`on_front_end_stack`]); reserved, committed only as used.
///
/// Operator chains and suffix chains are walked in loops ([LQ/grammar-v1.ebnf §P.13]: "keep their stacks on the
/// heap"), so recursion follows nesting alone, which the parser bounds at 64 levels. Recursive descent over 64 levels
/// still takes more stack than a caller's thread may have in an unoptimised build (about 22 KiB per level for the
/// parser and 13 KiB for the binder; a Windows main thread has 1 MiB), so a text that may nest deeper than
/// [`INLINE_NESTING`] levels is parsed and bound on a thread of its own. The printer and the encoders take at most about
/// 7 KiB per level and run on the caller's stack.
pub const FRONT_END_STACK: usize = 64 << 20;

/// The nesting a text may reach and still be parsed and bound on the caller's stack (see [`FRONT_END_STACK`]).
pub const INLINE_NESTING: u32 = 16;

thread_local! {
    /// Whether this thread is a front-end thread, whose nested parses and binds run in place.
    static ON_FRONT_END: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` on a thread with [`FRONT_END_STACK`] bytes of stack, or in place when this thread is one already, so a
/// call that nests others (a named query bound to learn a callee's columns) starts one thread at most.
pub fn on_front_end_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    if ON_FRONT_END.with(std::cell::Cell::get) {
        return f();
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("lq-front-end".into())
            .stack_size(FRONT_END_STACK)
            .spawn_scoped(scope, || {
                ON_FRONT_END.with(|c| c.set(true));
                f()
            })
            .expect("the front-end thread starts")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Runs `f`, which parses or binds `src`, in place when `src` nests at most [`INLINE_NESTING`] levels, else on the
/// front end's own stack ([`on_front_end_stack`]).
pub fn for_text<T: Send>(src: &str, f: impl FnOnce() -> T + Send) -> T {
    if nesting_bound(src) <= INLINE_NESTING {
        f()
    } else {
        on_front_end_stack(f)
    }
}

/// An upper bound of the nesting of [LQ/grammar-v1.ebnf §P.13] in a text, from one pass over its bytes: the deepest
/// bracket nesting outside strings, back-quoted names and comments, plus every `NOT` and `CASE` word (the entries that
/// have no bracket). Every other entry of P13 opens a bracket.
pub fn nesting_bound(src: &str) -> u32 {
    let b = src.as_bytes();
    let (mut depth, mut deepest, mut words) = (0u32, 0u32, 0u32);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' && q != b'`' { 2 } else { 1 };
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i + 1 < b.len() && (b[i + 1].is_ascii_alphanumeric() || b[i + 1] == b'_') {
                    i += 1;
                }
                let w = &b[start..=i];
                if w.eq_ignore_ascii_case(b"NOT") || w.eq_ignore_ascii_case(b"CASE") {
                    words += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    deepest.saturating_add(words)
}

/// Parses `read_input` (`moirai q`, MCP `query`) from raw bytes ([LQ/lexical §2.1] decoding first).
pub fn parse_read_bytes(bytes: &[u8], opts: ParseOptions) -> Result<Parsed<Read>, Vec<Diag>> {
    let src = decode(bytes).map_err(|d| vec![d])?;
    parse_read(src, opts)
}

/// Parses `read_input` from decoded text.
pub fn parse_read(src: &str, opts: ParseOptions) -> Result<Parsed<Read>, Vec<Diag>> {
    for_text(src, || {
        let mut p = Parser::new(src, opts, true);
        let r = p.read_input();
        p.finish(r)
    })
}

/// Parses `write_input` (`moirai tx`, MCP `write`): one `TX` block (P1).
pub fn parse_write(src: &str, opts: ParseOptions) -> Result<Parsed<Tx>, Vec<Diag>> {
    for_text(src, || {
        let mut p = Parser::new(src, opts, false);
        let r = p.write_input();
        p.finish(r)
    })
}

/// Parses a standard-library source with start symbol `define_stmt` (P1).
pub fn parse_define(src: &str, opts: ParseOptions) -> Result<Parsed<Define>, Vec<Diag>> {
    for_text(src, || {
        let mut p = Parser::new(src, opts, false);
        let r = p.define_input();
        p.finish(r)
    })
}

/// The marker of a failed parse; the diagnostic is in the parser.
#[derive(Debug)]
struct Fail;

type P<T> = Result<T, Fail>;

/// The reserved words ([LQ/lexical §6.1]).
pub const RESERVED: [&str; 44] = [
    "MATCH",
    "OPTIONAL",
    "WHERE",
    "WITH",
    "RETURN",
    "CALL",
    "YIELD",
    "UNWIND",
    "USE",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "ORDER",
    "BY",
    "LIMIT",
    "GROUP",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "TRUE",
    "FALSE",
    "EXISTS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "AS",
    "DISTINCT",
    "ASC",
    "DESC",
    "ASCENDING",
    "DESCENDING",
    "TX",
    "SET",
    "REMOVE",
    "DELETE",
    "CREATE",
    "INSERT",
    "EXPECT",
    "ASSERT",
];

/// Whether a word is reserved (ASCII case-insensitive).
pub fn is_reserved(word: &str) -> bool {
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(word))
}

/// The contextual keywords ([LQ/lexical §6.2]).
pub const CONTEXTUAL: [&str; 54] = [
    "EXPLAIN",
    "PROFILE",
    "ALL",
    "WALK",
    "TRAIL",
    "ACYCLIC",
    "SIMPLE",
    "DIFFERENT",
    "RELATIONSHIPS",
    "EDGES",
    "STARTS",
    "ENDS",
    "CONTAINS",
    "COUNT",
    "ANY",
    "NONE",
    "SIZE",
    "ON",
    "IF",
    "TIP",
    "TARGETS",
    "KEY",
    "LEASE",
    "MESSAGE",
    "DRY",
    "MOVE",
    "UNDER",
    "BEFORE",
    "AFTER",
    "FIRST",
    "LAST",
    "REOPEN",
    "REASON",
    "PATCH",
    "ADD",
    "POLICY",
    "RESTRICT",
    "CASCADE",
    "REPARENT",
    "REPLACED",
    "RELEASE",
    "UNLESS",
    "RESOLVE",
    "TAKE",
    "OURS",
    "THEIRS",
    "BASE",
    "VALUE",
    "REPOINT",
    "DEFINE",
    "QUERY",
    "SHAPE",
    "BUDGET",
    "DROP",
];

/// Clause keywords that start a reading clause.
const CLAUSE_START: [&str; 5] = ["MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH"];

/// The relations whose arguments hold revision positions ([LQ/lexical §4.2]): (name, positional argument 0 is a
/// revision, named revision arguments).
const REV_RELATIONS: [(&str, bool, &[&str]); 6] = [
    ("diff", true, &["range"]),
    ("log", true, &["range"]),
    ("changes", false, &["since", "ref"]),
    ("history", false, &["in"]),
    ("across", false, &["refs"]),
    ("violations", true, &["ref"]),
];

/// A parser position to backtrack to. Tokens are a pure function of their byte position, so the lookahead buffer is
/// not saved: restoring empties it and the next peek lexes again from `pos`.
#[derive(Clone, Copy)]
struct Saved {
    pos: usize,
    toks: usize,
    depth: u32,
    last_end: usize,
}

struct Parser<'a> {
    lx: Lexer<'a>,
    src: &'a str,
    pos: usize,
    /// The lookahead: normal-mode tokens lexed from `pos` on, not yet consumed.
    buf: VecDeque<Token>,
    last_end: usize,
    depth: u32,
    strict: bool,
    in_read: bool,
    toks: Vec<TokOut>,
    err: Option<Diag>,
    lexical: bool,
    errors: Vec<Diag>,
    last_recovery: Option<usize>,
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

fn span(a: usize, b: usize) -> Span {
    Span::new(a as u32, b as u32)
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, opts: ParseOptions, in_read: bool) -> Parser<'a> {
        Parser {
            lx: Lexer::new(src),
            src,
            pos: 0,
            buf: VecDeque::new(),
            last_end: 0,
            depth: 0,
            strict: opts.strict_gql,
            in_read,
            toks: Vec::new(),
            err: None,
            lexical: false,
            errors: Vec::new(),
            last_recovery: None,
        }
    }

    fn finish<T>(mut self, r: P<T>) -> Result<Parsed<T>, Vec<Diag>> {
        if let Err(Fail) = &r
            && let Some(d) = self.err.take()
        {
            self.push_error(d);
        }
        match r {
            Ok(tree) if self.errors.is_empty() => {
                self.toks.push(TokOut {
                    kind: "EOF",
                    value: "-".into(),
                });
                Ok(Parsed {
                    tree,
                    tokens: self.toks,
                })
            }
            _ => {
                if self.errors.is_empty() {
                    let at = self.pos as u32;
                    self.errors
                        .push(Diag::new(Code::E001, Span::new(at, at), "syntax error"));
                }
                self.errors.truncate(MAX_ERRORS);
                Err(self.errors)
            }
        }
    }

    fn push_error(&mut self, d: Diag) {
        let dup = self.errors.iter().any(|e| {
            e.code == d.code && e.span.map(|s| (s.start, s.end)) == d.span.map(|s| (s.start, s.end))
        });
        if !dup && self.errors.len() < MAX_ERRORS {
            self.errors.push(d);
        }
    }

    // ----- tokens --------------------------------------------------------------------------------------------------

    fn fail_d<T>(&mut self, d: Diag) -> P<T> {
        if self.err.is_none() {
            self.err = Some(d);
        }
        Err(Fail)
    }

    /// Token `n` of the lookahead (a copy: tokens hold only a kind and a range).
    fn la(&mut self, n: usize) -> P<Token> {
        while self.buf.len() <= n {
            let from = match self.buf.back() {
                Some(t) if t.kind == TokKind::Eof => {
                    let t = *t;
                    self.buf.push_back(t);
                    continue;
                }
                Some(t) => t.end,
                None => self.pos,
            };
            match self.lx.token(from) {
                Ok(t) => self.buf.push_back(t),
                Err(d) => {
                    self.lexical = true;
                    return self.fail_d(d);
                }
            }
        }
        Ok(self.buf[n])
    }

    fn save(&self) -> Saved {
        Saved {
            pos: self.pos,
            toks: self.toks.len(),
            depth: self.depth,
            last_end: self.last_end,
        }
    }

    fn restore(&mut self, s: Saved) {
        self.pos = s.pos;
        self.buf.clear();
        self.toks.truncate(s.toks);
        self.depth = s.depth;
        self.last_end = s.last_end;
    }

    fn text(&self, t: &Token) -> &'a str {
        &self.src[t.start..t.end]
    }

    fn is_punct(&mut self, n: usize, p: Punct) -> P<bool> {
        Ok(self.la(n)?.kind == TokKind::Punct(p))
    }

    fn is_word(&mut self, n: usize) -> P<bool> {
        Ok(self.la(n)?.kind == TokKind::Word)
    }

    fn is_kw(&mut self, n: usize, kw: &str) -> P<bool> {
        let t = self.la(n)?;
        Ok(t.kind == TokKind::Word && self.text(&t).eq_ignore_ascii_case(kw))
    }

    fn is_kw_any(&mut self, n: usize, kws: &[&str]) -> P<bool> {
        let t = self.la(n)?;
        Ok(t.kind == TokKind::Word && kws.iter().any(|k| self.text(&t).eq_ignore_ascii_case(k)))
    }

    fn rec(&mut self, kind: &'static str, value: Cow<'static, str>) {
        self.toks.push(TokOut { kind, value });
    }

    fn consume(&mut self) -> P<Token> {
        let t = self.la(0)?;
        self.buf.pop_front();
        self.pos = t.end;
        self.last_end = t.end;
        Ok(t)
    }

    /// Consumes one token and records it by its kind (words as `NAME`).
    fn bump(&mut self) -> P<Token> {
        let t = self.consume()?;
        let (kind, value): (&'static str, Cow<'static, str>) = match &t.kind {
            TokKind::Word => ("NAME", self.text(&t).to_string().into()),
            TokKind::QIdent => ("QNAME", json_string(&self.lx.qident_value(&t)).into()),
            TokKind::Param => ("PARAM", self.src[t.start + 1..t.end].to_string().into()),
            TokKind::Int(n) => ("INT", n.to_string().into()),
            TokKind::Float => ("FLOAT", self.text(&t).to_string().into()),
            TokKind::Dur(_) => ("DUR", self.text(&t).to_string().into()),
            TokKind::Str => ("STR", json_string(&self.lx.string_value(&t)).into()),
            TokKind::Node(n) => ("NODE", n.to_string().into()),
            TokKind::Uid => ("UID", self.src[t.start + 3..t.end].to_string().into()),
            TokKind::Punct(p) => ("P", p.as_str().into()),
            TokKind::Eof => return Ok(t),
        };
        self.rec(kind, value);
        Ok(t)
    }

    /// Consumes a string or back-quoted-name token, records it and returns its decoded value (decoded once).
    fn bump_decoded(&mut self) -> P<String> {
        let t = self.consume()?;
        let (kind, value) = match t.kind {
            TokKind::QIdent => ("QNAME", self.lx.qident_value(&t)),
            _ => ("STR", self.lx.string_value(&t)),
        };
        self.rec(kind, json_string(&value).into());
        Ok(value)
    }

    /// Consumes one token and records it as a keyword, in upper case.
    fn bump_kw(&mut self) -> P<Token> {
        let t = self.consume()?;
        let w = self.text(&t);
        let v = match RESERVED
            .iter()
            .chain(CONTEXTUAL.iter())
            .find(|k| k.eq_ignore_ascii_case(w))
        {
            Some(k) => Cow::Borrowed(*k),
            None => Cow::Owned(upper(w)),
        };
        self.rec("KW", v);
        Ok(t)
    }

    fn found(&self, t: &Token) -> String {
        match t.kind {
            TokKind::Eof => "end of input".to_string(),
            _ => q(self.text(t)),
        }
    }

    fn unexpected<T>(&mut self, expected: &[&str]) -> P<T> {
        let t = self.la(0)?;
        let shown: Vec<String> = expected.iter().take(5).map(|e| format!("`{e}`")).collect();
        let mut list = shown.join(", ");
        if expected.len() > 5 {
            list.push_str(", ...");
        }
        let mut d = Diag::new(
            Code::E001,
            t.span(),
            format!("expected {list}, found {}", self.found(&t)),
        );
        d.expected = expected.iter().take(10).map(|s| s.to_string()).collect();
        self.fail_d(d)
    }

    fn expect_punct(&mut self, p: Punct) -> P<Token> {
        if self.is_punct(0, p)? {
            self.bump()
        } else {
            self.unexpected(&[p.as_str()])
        }
    }

    fn expect_kw(&mut self, kw: &str) -> P<Token> {
        if self.is_kw(0, kw)? {
            self.bump_kw()
        } else {
            self.unexpected(&[kw])
        }
    }

    fn e004<T>(&mut self, at: Span, form: &str, inline: &str) -> P<T> {
        let d = Diag::new(Code::E004, at, format!("{} is not in LQ", q(form))).inline(inline);
        self.fail_d(d)
    }

    fn strict_refuse<T>(&mut self, at: Span, form: &str, gql: &str) -> P<T> {
        let d = Diag::new(
            Code::E004,
            at,
            format!(
                "{} is Cypher spelling; this surface takes the GQL spelling",
                q(form)
            ),
        )
        .inline(format!("write {gql}"));
        self.fail_d(d)
    }

    fn enter(&mut self, at: Span) -> P<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            let d = Diag::new(Code::E001, at, TOO_DEEP)
                .help("split the query or flatten the expression");
            return self.fail_d(d);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// A name in a plain-name position: any word or a back-quoted identifier ([LQ/lexical §6.3]).
    fn plain_name(&mut self) -> P<Name> {
        let t = self.la(0)?;
        match &t.kind {
            TokKind::Word => {
                self.bump()?;
                Ok(Name::new(self.text(&t), t.span()))
            }
            TokKind::QIdent => Ok(Name::new(self.bump_decoded()?, t.span())),
            _ => self.unexpected(&["name"]),
        }
    }

    /// Whether token `n` can be a variable: a non-reserved word or a back-quoted identifier.
    fn is_var(&mut self, n: usize) -> P<bool> {
        let t = self.la(n)?;
        Ok(match t.kind {
            TokKind::Word => !is_reserved(self.text(&t)),
            TokKind::QIdent => true,
            _ => false,
        })
    }

    /// A variable, alias or other non-plain name: a reserved word must be back-quoted ([LQ/lexical §6.3]).
    fn var_name(&mut self) -> P<Name> {
        if self.is_var(0)? {
            self.plain_name()
        } else {
            self.unexpected(&["name"])
        }
    }

    // ----- entry points --------------------------------------------------------------------------------------------

    fn read_input(&mut self) -> P<Read> {
        let mode = if self.is_kw(0, "EXPLAIN")? {
            self.bump_kw()?;
            Mode::Explain
        } else if self.is_kw(0, "PROFILE")? {
            self.bump_kw()?;
            Mode::Profile
        } else {
            Mode::Run
        };
        let query = self.query()?;
        self.end_of_read(&query)?;
        Ok(Read { mode, query })
    }

    fn write_input(&mut self) -> P<Tx> {
        if !self.is_kw(0, "TX")? {
            let t = self.la(0)?;
            let d = Diag::new(
                Code::E001,
                t.span(),
                "moirai tx and the write tool take one TX { ... } block",
            )
            .help("reads go through moirai q or the query tool");
            return self.fail_d(d);
        }
        let tx = self.tx()?;
        let t = self.la(0)?;
        if t.kind != TokKind::Eof {
            return self.second_statement(&t);
        }
        Ok(tx)
    }

    fn define_input(&mut self) -> P<Define> {
        if !self.is_kw(0, "DEFINE")? {
            return self.unexpected(&["DEFINE"]);
        }
        let d = self.define()?;
        let t = self.la(0)?;
        if t.kind != TokKind::Eof {
            return self.second_statement(&t);
        }
        Ok(d)
    }

    fn second_statement<T>(&mut self, t: &Token) -> P<T> {
        let d = Diag::new(
            Code::E005,
            t.span(),
            "one statement per call; a second statement starts here",
        )
        .help("send each statement in its own call, or put writes in one TX { ... } block");
        self.fail_d(d)
    }

    /// P17: what may follow a complete `read_input`.
    fn end_of_read(&mut self, query: &Query) -> P<()> {
        let t = self.la(0)?;
        if t.kind == TokKind::Eof {
            return Ok(());
        }
        if t.kind == TokKind::Punct(Punct::Semi) {
            return self.second_statement(&t);
        }
        if t.kind == TokKind::Word {
            let w = self.text(&t);
            if CLAUSE_START
                .iter()
                .chain(["RETURN", "USE", "EXPLAIN", "PROFILE"].iter())
                .any(|k| k.eq_ignore_ascii_case(w))
            {
                return self.second_statement(&t);
            }
            if w.eq_ignore_ascii_case("WHERE") {
                return self.where_after_return(query, &t);
            }
            if w.eq_ignore_ascii_case("SKIP") || w.eq_ignore_ascii_case("OFFSET") {
                return self.e004(
                    t.span(),
                    &format!("{} n", upper(w)),
                    "use the next cursor: --cursor K (the query tool: cursor)",
                );
            }
            if let Some(what) = self.write_keyword(0)? {
                return self.e006(t.span(), &what);
            }
        }
        self.unexpected(&["end of input", "UNION", "EXCEPT", "INTERSECT"])
    }

    fn where_after_return<T>(&mut self, query: &Query, t: &Token) -> P<T> {
        let items = match query.parts.last().map(|p| &p.body) {
            Some(PartBody::Clauses { ret, .. }) => printer::proj_items_text(ret.star, &ret.items),
            _ => "<items>".to_string(),
        };
        let saved = self.save();
        let err = self.err.take();
        self.bump_kw().ok();
        let cond = self.expr().ok().map(|e| printer::expr_text(&e));
        self.restore(saved);
        self.err = err;
        let inline = format!(
            "write WITH {items} WHERE {} RETURN {items}",
            cond.as_deref().unwrap_or("<expr>")
        );
        let d = Diag::new(Code::E001, t.span(), "WHERE after RETURN filters nothing")
            .inline(inline)
            .help("filter rows or groups with WITH ... WHERE before RETURN");
        self.fail_d(d)
    }

    fn e006<T>(&mut self, at: Span, what: &str) -> P<T> {
        let d = Diag::new(
            Code::E006,
            at,
            format!("{what} writes; q and the query tool only read"),
        )
        .help("send writes with moirai tx or the write tool");
        self.fail_d(d)
    }

    /// The write keyword at token `n`, as E006 names it, when the parse is a read ([LQ/grammar-v1.ebnf §R]).
    fn write_keyword(&mut self, n: usize) -> P<Option<String>> {
        if !self.in_read || !self.is_word(n)? {
            return Ok(None);
        }
        let t = self.la(n)?;
        let w = upper(self.text(&t));
        const WRITES: [&str; 12] = [
            "SET", "REMOVE", "DELETE", "CREATE", "INSERT", "MOVE", "REOPEN", "PATCH", "RESOLVE",
            "DEFINE", "ASSERT", "TX",
        ];
        if WRITES.contains(&w.as_str()) {
            if (w == "CREATE") && self.is_kw_any(n + 1, &["INDEX", "CONSTRAINT"])? {
                return Ok(None);
            }
            return Ok(Some(w));
        }
        if w == "DROP" && self.is_kw(n + 1, "QUERY")? {
            return Ok(Some("DROP QUERY".into()));
        }
        Ok(None)
    }

    // ----- queries -------------------------------------------------------------------------------------------------

    fn query(&mut self) -> P<Query> {
        let start = self.la(0)?.start;
        let mut parts = vec![self.single_query()?];
        let mut ops = Vec::new();
        loop {
            let op = if self.is_kw(0, "UNION")? {
                self.bump_kw()?;
                if self.is_kw(0, "ALL")? {
                    self.bump_kw()?;
                    SetOp::UnionAll
                } else {
                    SetOp::Union
                }
            } else if self.is_kw(0, "EXCEPT")? {
                self.bump_kw()?;
                SetOp::Except
            } else if self.is_kw(0, "INTERSECT")? {
                self.bump_kw()?;
                SetOp::Intersect
            } else {
                break;
            };
            ops.push(op);
            parts.push(self.single_query()?);
        }
        Ok(Query {
            parts,
            ops,
            span: span(start, self.last_end),
        })
    }

    /// A revision after `USE`, `ON` or `IF TIP` (a `revspec`, P2).
    fn revspec_here(&mut self) -> P<Rev> {
        self.buf.clear();
        match self.lx.revision(self.pos, false, &mut self.toks) {
            Ok((RevRead::Rev(r), end)) => {
                self.pos = end;
                self.last_end = end;
                Ok(r)
            }
            Ok(_) => unreachable!("a revspec position reads one revspec"),
            Err(d) => {
                self.lexical = true;
                self.fail_d(d)
            }
        }
    }

    fn single_query(&mut self) -> P<Part> {
        let start = self.la(0)?.start;
        let use_ = if self.is_kw(0, "USE")? {
            self.bump_kw()?;
            Some(self.revspec_here()?)
        } else {
            None
        };
        let mut clauses = Vec::new();
        if self.is_kw(0, "CALL")? && !self.is_punct(1, Punct::LBrace)? {
            let head = self.call_head()?;
            if self.starts_clause_or_return()? {
                clauses.push(Clause::Call(self.call_clause_of(head)?));
            } else {
                let (order, limit) = self.order_limit()?;
                let (proc, args, yield_, where_, s) = head;
                return Ok(Part {
                    use_,
                    body: PartBody::Call(SCall {
                        proc,
                        args,
                        yield_,
                        where_,
                        order,
                        limit,
                        span: span(s, self.last_end),
                    }),
                    span: span(start, self.last_end),
                });
            }
        }
        loop {
            let saved_depth = self.depth;
            match self.clause_or_return() {
                Ok(Some(c)) => clauses.push(c),
                Ok(None) => break,
                Err(Fail) => {
                    self.depth = saved_depth;
                    if !self.recover(&["MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH", "RETURN"]) {
                        return Err(Fail);
                    }
                }
            }
        }
        let ret = self.return_clause()?;
        Ok(Part {
            use_,
            body: PartBody::Clauses { clauses, ret },
            span: span(start, self.last_end),
        })
    }

    /// Panic-mode recovery (P14): records the pending error and skips to one of `sync` at bracket depth 0. Returns
    /// false when the pass must end (a lexical error, the error limit, or the end of the input).
    fn recover(&mut self, sync: &[&str]) -> bool {
        let Some(d) = self.err.take() else {
            return false;
        };
        self.push_error(d);
        if self.lexical || self.errors.len() >= MAX_ERRORS {
            return false;
        }
        let mut level = 0i32;
        // Always make progress: a recovery that would stop where the previous one stopped skips one token first.
        let here = match self.la(0) {
            Ok(t) => t,
            Err(Fail) => {
                if let Some(d) = self.err.take() {
                    self.push_error(d);
                }
                return false;
            }
        };
        if here.kind == TokKind::Eof {
            return false;
        }
        if self.last_recovery == Some(here.start) {
            match here.kind {
                TokKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => level += 1,
                TokKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => level -= 1,
                _ => {}
            }
            if self.consume().is_err() {
                return false;
            }
        }
        loop {
            let t = match self.la(0) {
                Ok(t) => t,
                Err(Fail) => {
                    if let Some(d) = self.err.take() {
                        self.push_error(d);
                    }
                    return false;
                }
            };
            match &t.kind {
                TokKind::Eof => return false,
                TokKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => level += 1,
                TokKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => {
                    if level == 0 && sync.contains(&"}") && t.kind == TokKind::Punct(Punct::RBrace)
                    {
                        self.last_recovery = Some(t.start);
                        return true;
                    }
                    level -= 1;
                }
                TokKind::Punct(Punct::Semi) if level == 0 && sync.contains(&";") => {
                    self.last_recovery = Some(t.start);
                    return true;
                }
                TokKind::Word if level <= 0 => {
                    let w = self.text(&t);
                    if sync.iter().any(|s| s.eq_ignore_ascii_case(w)) {
                        self.last_recovery = Some(t.start);
                        return true;
                    }
                }
                _ => {}
            }
            if self.consume().is_err() {
                return false;
            }
        }
    }

    fn starts_clause_or_return(&mut self) -> P<bool> {
        let mut kws: Vec<&str> = CLAUSE_START.to_vec();
        kws.push("RETURN");
        self.is_kw_any(0, &kws)
    }

    /// One reading clause, or `None` at `RETURN`. Refused and misplaced words at a clause position raise their codes.
    fn clause_or_return(&mut self) -> P<Option<Clause>> {
        if self.is_kw(0, "RETURN")? {
            return Ok(None);
        }
        if self.is_kw(0, "MATCH")? || self.is_kw(0, "OPTIONAL")? {
            return Ok(Some(Clause::Match(self.match_clause()?)));
        }
        if self.is_kw(0, "CALL")? && !self.is_punct(1, Punct::LBrace)? {
            let head = self.call_head()?;
            return Ok(Some(Clause::Call(self.call_clause_of(head)?)));
        }
        if self.is_kw(0, "UNWIND")? {
            return Ok(Some(Clause::Unwind(self.unwind()?)));
        }
        if self.is_kw(0, "WITH")? {
            return Ok(Some(Clause::With(self.with_clause()?)));
        }
        if self.is_kw(0, "USE")? {
            let t = self.bump_kw()?;
            let rev_text = {
                let mut scratch = Vec::new();
                self.buf.clear();
                match self.lx.revision(self.pos, false, &mut scratch) {
                    Ok((_, end)) => {
                        self.src[self.lx.skip_trivia(self.pos).unwrap_or(self.pos)..end].to_string()
                    }
                    Err(_) => "<revspec>".to_string(),
                }
            };
            let d = Diag::new(Code::E001, t.span(), "USE must start its query part")
                .inline(format!("write USE {rev_text} at the start of the part"));
            return self.fail_d(d);
        }
        self.clause_position_refusals(true)?;
        self.unexpected(&["MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH", "RETURN"])
    }

    /// The refused forms detected at a clause or statement position ([LQ/grammar-v1.ebnf §R]); returns normally when
    /// token 0 is none of them. `clause` is true at a reading-clause position.
    fn clause_position_refusals(&mut self, clause: bool) -> P<()> {
        let t = self.la(0)?;
        if t.kind != TokKind::Word {
            return Ok(());
        }
        let w = upper(self.text(&t));
        let sp = t.span();
        match w.as_str() {
            "MERGE" => return self.e004(sp, "MERGE", "write CREATE (...) UNLESS EXISTS { ... }"),
            "DETACH" | "NODETACH" => {
                return self.e004(
                    sp,
                    &format!("{w} DELETE"),
                    "write DELETE <x>; the schema's per-edge policies run (add POLICY or REPLACED BY)",
                );
            }
            "CREATE" | "DROP" if self.is_kw_any(1, &["INDEX", "CONSTRAINT"])? => {
                let t1 = self.la(1)?;
                let form = format!("{w} {}", upper(self.text(&t1)));
                return self.e004(sp, &form, "use CALL schema(); write data with moirai apply");
            }
            _ => {}
        }
        if !clause {
            return Ok(());
        }
        match w.as_str() {
            "CALL" if self.is_punct(1, Punct::LBrace)? => {
                self.e004(sp, "CALL { ... }", "use EXISTS { }, COUNT { } or WITH")
            }
            "NEXT" => self.e004(sp, "NEXT", "use WITH"),
            "FOREACH" => self.e004(sp, "FOREACH", "use one SET per MATCH target"),
            "LOAD" | "SHOW" => self.e004(
                sp,
                &format!("{w} ..."),
                "use CALL schema(); write data with moirai apply",
            ),
            "FOR" => {
                let inline = self
                    .rewrite_for()
                    .unwrap_or_else(|| "write UNWIND <list> AS <x>".into());
                self.e004(sp, "FOR x IN list", &inline)
            }
            "LET" => {
                let inline = self
                    .rewrite_let()
                    .unwrap_or_else(|| "write WITH *, <e> AS <x>".into());
                self.e004(sp, "LET x = e", &inline)
            }
            "FILTER" => {
                let inline = self
                    .rewrite_filter()
                    .unwrap_or_else(|| "write WITH * WHERE <e>".into());
                self.e004(sp, "FILTER e", &inline)
            }
            _ => {
                if let Some(what) = self.write_keyword(0)? {
                    return self.e006(sp, &what);
                }
                Ok(())
            }
        }
    }

    /// Runs `f` speculatively and restores the parser; the pending error is kept.
    fn peek_parse<T>(&mut self, f: impl FnOnce(&mut Self) -> P<T>) -> Option<T> {
        let saved = self.save();
        let err = self.err.take();
        let lexical = self.lexical;
        let r = f(self).ok();
        self.restore(saved);
        self.err = err;
        self.lexical = lexical;
        r
    }

    fn source_of(&self, s: Span) -> String {
        self.src[s.start as usize..s.end as usize].to_string()
    }

    fn rewrite_for(&mut self) -> Option<String> {
        self.peek_parse(|p| {
            p.consume()?;
            let x = p.var_name()?;
            p.expect_kw("IN")?;
            let e = p.expr()?;
            Ok(format!(
                "write UNWIND {} AS {}",
                p.source_of(e.span),
                x.text
            ))
        })
    }

    fn rewrite_let(&mut self) -> Option<String> {
        self.peek_parse(|p| {
            p.consume()?;
            let x = p.var_name()?;
            p.expect_punct(Punct::Eq)?;
            let e = p.expr()?;
            Ok(format!(
                "write WITH *, {} AS {}",
                p.source_of(e.span),
                x.text
            ))
        })
    }

    fn rewrite_filter(&mut self) -> Option<String> {
        self.peek_parse(|p| {
            p.consume()?;
            if p.is_kw(0, "WHERE")? {
                p.consume()?;
            }
            let e = p.expr()?;
            Ok(format!("write WITH * WHERE {}", p.source_of(e.span)))
        })
    }

    fn match_clause(&mut self) -> P<Match> {
        let start = self.la(0)?.start;
        let optional = if self.is_kw(0, "OPTIONAL")? {
            self.bump_kw()?;
            self.expect_kw("MATCH")?;
            true
        } else {
            self.bump_kw()?;
            false
        };
        let mut mode = None;
        if !optional && self.is_word(0)? && !self.is_punct(1, Punct::Eq)? {
            let t = self.la(0)?;
            let w = upper(self.text(&t));
            match w.as_str() {
                "WALK" | "TRAIL" | "ACYCLIC" | "SIMPLE" => {
                    self.bump_kw()?;
                    mode = Some(match w.as_str() {
                        "WALK" => MatchMode::Walk,
                        "TRAIL" => MatchMode::Trail,
                        "ACYCLIC" => MatchMode::Acyclic,
                        _ => MatchMode::Simple,
                    });
                }
                "DIFFERENT" => {
                    self.bump_kw()?;
                    if self.is_kw_any(0, &["RELATIONSHIPS", "EDGES"])? {
                        self.bump_kw()?;
                    } else {
                        return self.unexpected(&["RELATIONSHIPS", "EDGES"]);
                    }
                    mode = Some(MatchMode::Different);
                }
                "REPEATABLE" => {
                    return self.e004(
                        t.span(),
                        "REPEATABLE ELEMENTS",
                        "remove it: patterns bind distinct edges and quantified parts bind endpoint pairs",
                    );
                }
                "SHORTEST" | "ANY" | "ALL" => {
                    return self.e004(
                        t.span(),
                        &w,
                        "remove the selector: quantified parts bind endpoint pairs",
                    );
                }
                _ => {}
            }
        }
        let patterns = self.pattern_list()?;
        let where_ = self.opt_where()?;
        Ok(Match {
            optional,
            mode,
            patterns,
            where_,
            span: span(start, self.last_end),
        })
    }

    fn opt_where(&mut self) -> P<Option<Expr>> {
        if self.is_kw(0, "WHERE")? {
            self.bump_kw()?;
            Ok(Some(self.expr()?))
        } else {
            Ok(None)
        }
    }

    /// `CALL proc_name ( args ) [ YIELD ( * | items ) [ WHERE expr ] ]`.
    #[allow(clippy::type_complexity)]
    fn call_head(&mut self) -> P<(Name, Vec<Arg>, YieldMode, Option<Expr>, usize)> {
        let start = self.bump_kw()?.start;
        let proc = self.proc_name(false)?;
        self.expect_punct(Punct::LParen)?;
        let args = self.args(Some(&proc.text))?;
        let mut yield_ = YieldMode::None;
        let mut where_ = None;
        if self.is_kw(0, "YIELD")? {
            self.bump_kw()?;
            if self.is_punct(0, Punct::Star)? {
                self.bump()?;
                yield_ = YieldMode::Star;
            } else {
                yield_ = YieldMode::Items(self.yield_items()?);
            }
            where_ = self.opt_where()?;
        }
        Ok((proc, args, yield_, where_, start))
    }

    /// P7: a CALL followed by more clauses (or any CALL after the first clause) needs `YIELD` with named items.
    fn call_clause_of(
        &mut self,
        head: (Name, Vec<Arg>, YieldMode, Option<Expr>, usize),
    ) -> P<Call> {
        let (proc, args, yield_, where_, start) = head;
        match yield_ {
            YieldMode::Items(items) => Ok(Call {
                proc,
                args,
                yield_: items,
                where_,
                span: span(start, self.last_end),
            }),
            _ => {
                let at = self.la(0)?.span();
                let d = Diag::new(
                    Code::E001,
                    at,
                    "a CALL followed by more clauses needs YIELD with named columns",
                );
                self.fail_d(d)
            }
        }
    }

    /// `proc_name` (or `tx_name` inside `TX`): segments joined with `.`; the first is a plain name.
    fn proc_name(&mut self, in_tx: bool) -> P<Name> {
        let first = self.la(0)?;
        let first_text = match &first.kind {
            TokKind::Word => self.text(&first).to_string(),
            TokKind::QIdent => self.lx.qident_value(&first),
            _ => return self.unexpected(&["name"]),
        };
        if first.kind == TokKind::Word {
            let lw = first_text.to_ascii_lowercase();
            if ["apoc", "gds", "db", "dbms"].contains(&lw.as_str())
                && self.is_punct(1, Punct::Dot)?
            {
                return self.e004(
                    first.span(),
                    &format!("{first_text}.*"),
                    "use the built-in table functions; CALL schema() lists kinds, fields and edges",
                );
            }
        }
        // Every segment of a `proc_name` or `tx_name` is a plain-name position, the `tx` of `tx.complete` included
        // ([LQ/grammar-v1.ebnf §P.3], [LQ/lexical §6.3]): the token stream prints it `NAME`.
        let is_tx = first.kind == TokKind::Word && first_text.eq_ignore_ascii_case("tx");
        self.bump()?;
        let mut text = first_text;
        while self.is_punct(0, Punct::Dot)? {
            self.bump()?;
            let seg = self.plain_name()?;
            text.push('.');
            text.push_str(&seg.text);
        }
        let name = Name::new(text, span(first.start, self.last_end));
        if is_tx && self.in_read && !in_tx {
            return self.e006(name.span, &format!("CALL {}", name.text));
        }
        Ok(name)
    }

    fn yield_items(&mut self) -> P<Vec<YItem>> {
        let mut items = Vec::new();
        loop {
            let name = self.plain_name()?;
            let as_ = if self.is_kw(0, "AS")? {
                self.bump_kw()?;
                Some(self.var_name()?)
            } else {
                None
            };
            items.push(YItem { name, as_ });
            if self.is_punct(0, Punct::Comma)? {
                self.bump()?;
            } else {
                return Ok(items);
            }
        }
    }

    /// An argument list after `(` up to and including `)`. `proc` is the callee of a `CALL` (revision positions apply,
    /// [LQ/lexical §4.2]); `None` for a function call.
    fn args(&mut self, proc: Option<&str>) -> P<Vec<Arg>> {
        let rel = proc.and_then(|p| {
            REV_RELATIONS
                .iter()
                .find(|(n, _, _)| n.eq_ignore_ascii_case(p))
                .copied()
        });
        let mut args = Vec::new();
        if self.is_punct(0, Punct::RParen)? {
            self.bump()?;
            return Ok(args);
        }
        let mut positional = 0usize;
        loop {
            let mut name: Option<Name> = None;
            if rel.is_some() {
                if let Some((ws, we, _)) = self.lx.named_arg_at(self.pos) {
                    self.buf.clear();
                    let t = self.consume()?;
                    debug_assert_eq!((t.start, t.end), (ws, we));
                    self.rec("NAME", self.src[ws..we].to_string().into());
                    name = Some(Name::new(&self.src[ws..we], span(ws, we)));
                    self.bump()?; // ':'
                } else if matches!(
                    self.lx
                        .skip_trivia(self.pos)
                        .map(|i| self.src.as_bytes().get(i).copied()),
                    Ok(Some(b'`'))
                ) && matches!(self.la(0)?.kind, TokKind::QIdent)
                    && self.is_punct(1, Punct::Colon)?
                {
                    name = Some(self.plain_name()?);
                    self.bump()?;
                }
            } else if matches!(self.la(0)?.kind, TokKind::Word | TokKind::QIdent)
                && self.is_punct(1, Punct::Colon)?
            {
                name = Some(self.plain_name()?);
                self.bump()?;
            }
            let rev_pos = match (rel, &name) {
                (Some((_, _, named)), Some(n)) => named.contains(&n.text.as_str()),
                (Some((_, pos0, _)), None) => pos0 && positional == 0,
                _ => false,
            };
            if name.is_none() {
                positional += 1;
            }
            let value = if rev_pos {
                self.rev_arg()?
            } else {
                ArgVal::Expr(self.expr()?)
            };
            args.push(Arg { name, value });
            if self.is_punct(0, Punct::Comma)? {
                self.bump()?;
                continue;
            }
            self.expect_punct(Punct::RParen)?;
            return Ok(args);
        }
    }

    fn rev_arg(&mut self) -> P<ArgVal> {
        self.buf.clear();
        match self.lx.revision(self.pos, true, &mut self.toks) {
            Ok((RevRead::Quote, _)) => Ok(ArgVal::Expr(self.expr()?)),
            Ok((read, end)) => {
                self.pos = end;
                self.last_end = end;
                Ok(match read {
                    RevRead::Rev(r) => ArgVal::Rev(r),
                    RevRead::Range(from, op, to, span) => ArgVal::Range { from, op, to, span },
                    RevRead::List(elems, span) => ArgVal::List(elems, span),
                    RevRead::Quote => unreachable!("handled above"),
                })
            }
            Err(d) => {
                self.lexical = true;
                self.fail_d(d)
            }
        }
    }

    fn unwind(&mut self) -> P<Unwind> {
        let start = self.bump_kw()?.start;
        let expr = self.expr()?;
        self.expect_kw("AS")?;
        let as_ = self.var_name()?;
        Ok(Unwind {
            expr,
            as_,
            span: span(start, self.last_end),
        })
    }

    fn with_clause(&mut self) -> P<With> {
        let start = self.bump_kw()?.start;
        let distinct = if self.is_kw(0, "DISTINCT")? {
            self.bump_kw()?;
            true
        } else {
            false
        };
        let (star, items) = self.proj_items()?;
        let where_ = self.opt_where()?;
        let (order, limit) = self.order_limit()?;
        if (!order.is_empty() || limit.is_some()) && self.is_kw(0, "WHERE")? {
            let t = self.la(0)?;
            let items_text = printer::proj_items_text(star, &items);
            let keys = order
                .iter()
                .map(printer::sort_text)
                .collect::<Vec<_>>()
                .join(", ");
            let cond = self.peek_parse(|p| {
                p.consume()?;
                p.expr()
            });
            let mut inline = format!(
                "write WITH {items_text} WHERE {}",
                cond.map(|e| printer::expr_text(&e))
                    .unwrap_or("<expr>".into())
            );
            if !keys.is_empty() {
                inline.push_str(&format!(" ORDER BY {keys}"));
            }
            if let Some(l) = &limit {
                inline.push_str(&format!(" LIMIT {}", printer::expr_text(l)));
            }
            let d = Diag::new(
                Code::E001,
                t.span(),
                "WITH takes WHERE before ORDER BY and LIMIT",
            )
            .inline(inline);
            return self.fail_d(d);
        }
        Ok(With {
            distinct,
            star,
            items,
            where_,
            order,
            limit,
            span: span(start, self.last_end),
        })
    }

    fn proj_items(&mut self) -> P<(bool, Vec<Item>)> {
        let mut items = Vec::new();
        let star = if self.is_punct(0, Punct::Star)? {
            self.bump()?;
            true
        } else {
            items.push(self.item()?);
            false
        };
        while self.is_punct(0, Punct::Comma)? {
            self.bump()?;
            items.push(self.item()?);
        }
        Ok((star, items))
    }

    fn item(&mut self) -> P<Item> {
        let expr = self.expr()?;
        let as_ = if self.is_kw(0, "AS")? {
            self.bump_kw()?;
            Some(self.var_name()?)
        } else {
            None
        };
        Ok(Item { expr, as_ })
    }

    fn return_clause(&mut self) -> P<Return> {
        if !self.is_kw(0, "RETURN")? {
            return self.unexpected(&["RETURN"]);
        }
        let start = self.bump_kw()?.start;
        let mut distinct = false;
        if self.is_kw(0, "DISTINCT")? {
            self.bump_kw()?;
            distinct = true;
        } else if self.is_kw(0, "ALL")? && !self.is_punct(1, Punct::LParen)? {
            self.bump_kw()?;
        }
        let (star, items) = self.proj_items()?;
        let mut group = Vec::new();
        if self.is_kw(0, "GROUP")? {
            self.bump_kw()?;
            self.expect_kw("BY")?;
            group.push(self.expr()?);
            while self.is_punct(0, Punct::Comma)? {
                self.bump()?;
                group.push(self.expr()?);
            }
        }
        let (order, limit) = self.order_limit()?;
        Ok(Return {
            distinct,
            star,
            items,
            group,
            order,
            limit,
            span: span(start, self.last_end),
        })
    }

    fn skip_offset(&mut self) -> P<()> {
        if self.is_kw_any(0, &["SKIP", "OFFSET"])? {
            let t = self.la(0)?;
            let w = upper(self.text(&t));
            return self.e004(
                t.span(),
                &format!("{w} n"),
                "use the next cursor: --cursor K (the query tool: cursor)",
            );
        }
        Ok(())
    }

    fn order_limit(&mut self) -> P<(Vec<Sort>, Option<Expr>)> {
        self.skip_offset()?;
        let mut order = Vec::new();
        if self.is_kw(0, "ORDER")? {
            self.bump_kw()?;
            self.expect_kw("BY")?;
            loop {
                let expr = self.expr()?;
                let mut desc = false;
                if self.is_kw_any(0, &["ASC", "ASCENDING"])? {
                    self.bump_kw()?;
                } else if self.is_kw_any(0, &["DESC", "DESCENDING"])? {
                    self.bump_kw()?;
                    desc = true;
                }
                order.push(Sort { expr, desc });
                if self.is_punct(0, Punct::Comma)? {
                    self.bump()?;
                } else {
                    break;
                }
            }
        }
        self.skip_offset()?;
        let mut limit = None;
        if self.is_kw(0, "LIMIT")? {
            self.bump_kw()?;
            let t = self.la(0)?;
            limit = Some(match t.kind {
                TokKind::Int(n) => {
                    self.bump()?;
                    Expr::new(ExprKind::Int(n), t.span())
                }
                TokKind::Param => {
                    self.bump()?;
                    Expr::new(
                        ExprKind::Param(self.src[t.start + 1..t.end].to_string()),
                        t.span(),
                    )
                }
                _ => return self.unexpected(&["integer", "$param"]),
            });
        }
        self.skip_offset()?;
        Ok((order, limit))
    }

    // ----- patterns ------------------------------------------------------------------------------------------------

    /// The checks at the start of a pattern: a path variable (E113) and the refused path functions (E004).
    fn pattern_start(&mut self) -> P<()> {
        let t = self.la(0)?;
        if t.kind == TokKind::Word && !is_reserved(self.text(&t)) && self.is_punct(1, Punct::Eq)? {
            let path = self.peek_parse(|p| {
                p.consume()?;
                p.consume()?;
                p.path()
            });
            let id = path
                .and_then(|p| self.anchor_id(&p))
                .unwrap_or_else(|| "#N".into());
            let d = Diag::new(Code::E113, t.span(), "path variables are not in LQ v1")
                .inline(format!(
                    "write CALL blockers({id}, transitive: true) YIELD blocker, depth, via"
                ))
                .help("sets of endpoints need no path variable");
            return self.fail_d(d);
        }
        if t.kind == TokKind::Word && self.is_punct(1, Punct::LParen)? {
            self.refused_function(&t)?;
        }
        Ok(())
    }

    /// The anchor id of a path for E113's rewrite ([LQ/errors §5.2]: "with the pattern's anchor id, else `#N`"): the
    /// first of its endpoint node patterns, from its start, that names a node by a literal (`(#N)`, `(#u:…)`, `{id: #N}`,
    /// `{id: #u:…}`), as written.
    fn anchor_id(&self, path: &Path) -> Option<String> {
        std::iter::once(&path.start)
            .chain(path.steps.iter().map(|s| match s {
                Step::Edge(_, n) | Step::Group(_, n) => n,
            }))
            .flat_map(|n| n.props.iter())
            .find(|kv| {
                kv.key.text == "id" && matches!(kv.value.kind, ExprKind::Nid(_) | ExprKind::Uid(_))
            })
            .map(|kv| self.source_of(kv.value.span))
    }

    fn pattern_list(&mut self) -> P<Vec<Path>> {
        let mut paths = Vec::new();
        loop {
            self.pattern_start()?;
            paths.push(self.path()?);
            if self.is_punct(0, Punct::Comma)? {
                self.bump()?;
            } else {
                return Ok(paths);
            }
        }
    }

    fn path(&mut self) -> P<Path> {
        let start = self.node_pat(true)?;
        self.path_rest(start)
    }

    fn path_rest(&mut self, start: NPat) -> P<Path> {
        let s = start.span.start as usize;
        let mut steps = Vec::new();
        loop {
            if self.is_punct(0, Punct::LArrow)? || self.is_punct(0, Punct::Minus)? {
                let e = self.edge_pat()?;
                let n = self.node_pat(true)?;
                steps.push(Step::Edge(e, n));
            } else if self.is_punct(0, Punct::LParen)? {
                let g = self.group_pat()?;
                let n = self.node_pat(true)?;
                steps.push(Step::Group(g, n));
            } else {
                break;
            }
        }
        Ok(Path {
            start,
            steps,
            span: span(s, self.last_end),
        })
    }

    fn group_pat(&mut self) -> P<Group> {
        let open = self.la(0)?;
        self.enter(open.span())?;
        self.bump()?;
        let path = self.path()?;
        let where_ = self.opt_where()?;
        self.expect_punct(Punct::RParen)?;
        self.leave();
        if !(self.is_punct(0, Punct::Plus)?
            || self.is_punct(0, Punct::Star)?
            || self.is_punct(0, Punct::LBrace)?)
        {
            let at = self.la(0)?.span();
            let d = Diag::new(
                Code::E001,
                at,
                "a parenthesised path group needs a quantifier",
            )
            .inline("add +, * or {m,n} after the group");
            return self.fail_d(d);
        }
        let quant = self.quantifier()?;
        Ok(Group {
            path,
            where_,
            quant,
            span: span(open.start, self.last_end),
        })
    }

    /// `node_pat`; `pattern` is true in a pattern (E118 applies to its property map, P18).
    fn node_pat(&mut self, pattern: bool) -> P<NPat> {
        let open = self.expect_punct(Punct::LParen)?;
        let t = self.la(0)?;
        if matches!(t.kind, TokKind::Node(_) | TokKind::Uid) {
            let lit = self.node_literal()?;
            self.expect_punct(Punct::RParen)?;
            return Ok(NPat {
                var: None,
                labels: Vec::new(),
                props: vec![Kv {
                    key: Name::new("id", lit.span),
                    value: lit,
                }],
                where_: None,
                span: span(open.start, self.last_end),
            });
        }
        let var = if self.is_var(0)? {
            Some(self.plain_name()?)
        } else {
            None
        };
        let mut labels = Vec::new();
        if self.is_punct(0, Punct::Colon)? {
            self.bump()?;
            labels = self.label_expr()?;
        }
        let props = if self.is_punct(0, Punct::LBrace)? {
            self.prop_map(pattern, var.as_ref())?
        } else {
            Vec::new()
        };
        let where_ = self.opt_where()?;
        self.expect_punct(Punct::RParen)?;
        Ok(NPat {
            var,
            labels,
            props,
            where_,
            span: span(open.start, self.last_end),
        })
    }

    fn node_literal(&mut self) -> P<Expr> {
        let t = self.bump()?;
        Ok(match t.kind {
            TokKind::Node(n) => Expr::new(ExprKind::Nid(n), t.span()),
            _ => Expr::new(
                ExprKind::Uid(self.src[t.start + 3..t.end].to_string()),
                t.span(),
            ),
        })
    }

    fn label_expr(&mut self) -> P<Vec<Name>> {
        let mut labels = vec![self.plain_name()?];
        while self.is_punct(0, Punct::Pipe)? {
            self.bump()?;
            labels.push(self.plain_name()?);
        }
        if self.is_punct(0, Punct::Colon)? {
            let at = self.la(0)?.span();
            let second = self.peek_parse(|p| {
                p.consume()?;
                p.plain_name()
            });
            let b = second.map(|n| n.text).unwrap_or_else(|| "<b>".into());
            let inline = format!("write :{}|{} (a node has one kind)", labels[0].text, b);
            return self.e004(at, ":a:b", &inline);
        }
        Ok(labels)
    }

    /// `prop_map`; in a pattern a `NULL` value is E118 (P18).
    fn prop_map(&mut self, pattern: bool, var: Option<&Name>) -> P<Vec<Kv>> {
        let open = self.la(0)?;
        self.enter(open.span())?;
        self.bump()?;
        let mut entries = Vec::new();
        if !self.is_punct(0, Punct::RBrace)? {
            loop {
                let key = self.plain_name()?;
                self.expect_punct(Punct::Colon)?;
                let value = self.expr()?;
                if pattern && value.kind == ExprKind::Null {
                    let v = var.map_or("x", |v| v.text.as_str());
                    let d = Diag::new(
                        Code::E118,
                        key.span.to(value.span),
                        "a comparison with NULL is never true",
                    )
                    .inline(format!("write WHERE {v}.{} IS NULL", key.text));
                    return self.fail_d(d);
                }
                entries.push(Kv { key, value });
                if self.is_punct(0, Punct::Comma)? {
                    self.bump()?;
                } else {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RBrace)?;
        self.leave();
        Ok(entries)
    }

    fn edge_pat(&mut self) -> P<EPat> {
        let first = self.la(0)?;
        let mut body = EdgeBody::default();
        let dir;
        if first.kind == TokKind::Punct(Punct::LArrow) {
            self.bump()?;
            if self.is_punct(0, Punct::Arrow)? {
                let at = self.la(0)?.span();
                return self.e004(
                    first.span().to(at),
                    "<-->",
                    "write -[...]- (either direction)",
                );
            }
            if self.is_punct(0, Punct::LBracket)? {
                body = self.edge_body(true)?;
                if self.is_punct(0, Punct::Arrow)? {
                    let at = self.la(0)?.span();
                    return self.e004(
                        first.span().to(at),
                        "<-[...]->",
                        "write -[...]- (either direction)",
                    );
                }
                self.expect_punct(Punct::Minus)?;
            } else {
                self.expect_punct(Punct::Minus)?;
                if self.is_punct(0, Punct::Arrow)? {
                    let at = self.la(0)?.span();
                    return self.e004(
                        first.span().to(at),
                        "<-->",
                        "write -[...]- (either direction)",
                    );
                }
            }
            dir = Dir::Left;
        } else {
            self.expect_punct(Punct::Minus)?;
            if self.is_punct(0, Punct::LBracket)? {
                body = self.edge_body(false)?;
                if self.is_punct(0, Punct::Arrow)? {
                    self.bump()?;
                    dir = Dir::Right;
                } else if self.is_punct(0, Punct::Minus)? {
                    self.bump()?;
                    dir = Dir::Both;
                } else {
                    return self.unexpected(&["->", "-"]);
                }
            } else if self.is_punct(0, Punct::Arrow)? {
                self.bump()?;
                dir = Dir::Right;
            } else if self.is_punct(0, Punct::Minus)? {
                self.bump()?;
                dir = Dir::Both;
            } else {
                return self.unexpected(&["[", "->", "-"]);
            }
        }
        let mut quant = body.quant;
        if self.is_punct(0, Punct::Plus)?
            || self.is_punct(0, Punct::Star)?
            || self.is_punct(0, Punct::LBrace)?
        {
            let at = self.la(0)?.span();
            let q2 = self.quantifier()?;
            if quant.is_some() {
                let d = Diag::new(Code::E114, at, "one edge takes one quantifier");
                return self.fail_d(d);
            }
            quant = Some(q2);
        }
        Ok(EPat {
            var: body.var,
            dir,
            types: body.types,
            quant,
            props: body.props,
            where_: body.where_,
            span: span(first.start, self.last_end),
        })
    }

    /// `'[' edge_body ']'` up to and including `]`; `left` is true after `<-`.
    fn edge_body(&mut self, left: bool) -> P<EdgeBody> {
        self.bump()?; // '['
        let mut b = EdgeBody::default();
        if self.is_var(0)? {
            b.var = Some(self.plain_name()?);
        }
        if self.is_punct(0, Punct::Colon)? {
            self.bump()?;
            b.types.push(self.plain_name()?);
            while self.is_punct(0, Punct::Pipe)? {
                self.bump()?;
                b.types.push(self.plain_name()?);
            }
        }
        if self.is_punct(0, Punct::Star)? {
            let star = self.la(0)?;
            let s = star.start;
            self.bump()?;
            let mut lo: Option<(i64, Span)> = None;
            let mut hi: Option<(i64, Span)> = None;
            let mut dots = false;
            if let TokKind::Int(n) = self.la(0)?.kind {
                let t = self.bump()?;
                lo = Some((n, t.span()));
            }
            if self.is_punct(0, Punct::DotDot)? {
                self.bump()?;
                dots = true;
                if let TokKind::Int(n) = self.la(0)?.kind {
                    let t = self.bump()?;
                    hi = Some((n, t.span()));
                }
            }
            // Every E114 is located at the quantifier's first token, here its `*` (see [`Self::brace_quant`]).
            let qspan = span(s, self.last_end);
            for (n, _) in lo.iter().chain(hi.iter()) {
                if *n > u32::MAX as i64 {
                    let d = Diag::new(
                        Code::E114,
                        qspan,
                        format!("quantifier bound {n} is above 4294967295"),
                    );
                    return self.fail_d(d);
                }
            }
            let (min, max) = match (lo, dots, hi) {
                (None, false, _) => (1, None),
                (Some((m, _)), false, _) => (m as u32, Some(m as u32)),
                (Some((m, _)), true, Some((n, _))) => (m as u32, Some(n as u32)),
                (None, true, Some((n, _))) => (1, Some(n as u32)),
                (Some((m, _)), true, None) => (m as u32, None),
                (None, true, None) => (1, None),
            };
            if let Some(mx) = max
                && min > mx
            {
                let d = Diag::new(
                    Code::E114,
                    qspan,
                    format!("quantifier {{{min},{mx}}} has m > n"),
                );
                return self.fail_d(d);
            }
            let quant = Quant { min, max };
            if self.strict {
                let form = self.src[s..self.last_end].to_string();
                let gql = self.gql_edge(left, &b, quant);
                return self.strict_refuse(qspan, &form, &gql);
            }
            b.quant = Some(quant);
        }
        if self.is_punct(0, Punct::LBrace)? {
            b.props = self.prop_map(true, b.var.as_ref())?;
        }
        b.where_ = self.opt_where()?;
        self.expect_punct(Punct::RBracket)?;
        Ok(b)
    }

    /// The replacement the strict-GQL mode prints for a Cypher quantifier inside an edge's brackets
    /// ([LQ/grammar-v1.ebnf §G.2], [LQ/gql-spelling §3.3]): the whole edge with the quantifier after it
    /// (`<-[:BLOCKS*2..3]-` gives `<-[:BLOCKS]-{2,3}`). The rest of the edge is read ahead without consuming it; when it
    /// does not parse, the edge is printed from what precedes the quantifier.
    #[cold]
    #[inline(never)]
    fn gql_edge(&mut self, left: bool, b: &EdgeBody, quant: Quant) -> String {
        let (var, types) = (b.var.clone(), b.types.clone());
        let rest = self.peek_parse(|p| {
            let props = if p.is_punct(0, Punct::LBrace)? {
                p.prop_map(true, var.as_ref())?
            } else {
                Vec::new()
            };
            let where_ = p.opt_where()?;
            p.expect_punct(Punct::RBracket)?;
            let dir = if left {
                p.expect_punct(Punct::Minus)?;
                Dir::Left
            } else if p.is_punct(0, Punct::Arrow)? {
                Dir::Right
            } else {
                p.expect_punct(Punct::Minus)?;
                Dir::Both
            };
            Ok((props, where_, dir))
        });
        let (props, where_, dir) =
            rest.unwrap_or((Vec::new(), None, if left { Dir::Left } else { Dir::Right }));
        let e = EPat {
            var,
            dir,
            types,
            quant: Some(quant),
            props,
            where_,
            span: Span::default(),
        };
        printer::edge_text(&e, printer::Spelling::Gql)
    }

    /// A GQL quantifier after an edge or group: `+`, `*`, `{m,n}`, `{m,}`, `{m}`, `{,n}` (P6).
    fn quantifier(&mut self) -> P<Quant> {
        let t = self.la(0)?;
        if t.kind == TokKind::Punct(Punct::Plus) {
            self.bump()?;
            return Ok(Quant { min: 1, max: None });
        }
        if t.kind == TokKind::Punct(Punct::Star) {
            self.bump()?;
            return Ok(Quant { min: 0, max: None });
        }
        let saved = self.save();
        match self.brace_quant() {
            Ok(q) => Ok(q),
            Err(Fail) => {
                let quant_err = self.err.take();
                let lexical = self.lexical;
                self.restore(saved);
                if lexical {
                    self.err = quant_err;
                    return Err(Fail);
                }
                if let Some(d) = &quant_err
                    && d.code == Code::E114
                {
                    self.err = quant_err;
                    return Err(Fail);
                }
                // Scan the braces: integers only is E114, anything else E001.
                let mut i = 0;
                let mut ints = false;
                let mut other = false;
                let mut end = t.end;
                loop {
                    i += 1;
                    let x = match self.la(i) {
                        Ok(x) => x,
                        Err(Fail) => {
                            self.err = None;
                            self.lexical = false;
                            other = true;
                            break;
                        }
                    };
                    match x.kind {
                        TokKind::Punct(Punct::RBrace) => {
                            end = x.end;
                            break;
                        }
                        TokKind::Eof => {
                            other = true;
                            break;
                        }
                        TokKind::Int(_) => ints = true,
                        TokKind::Punct(
                            Punct::Comma | Punct::DotDot | Punct::Dot | Punct::Minus | Punct::Plus,
                        ) => {}
                        _ => other = true,
                    }
                    if i > 32 {
                        other = true;
                        break;
                    }
                }
                if ints && !other {
                    let text = &self.src[t.start + 1..end - 1];
                    let d = Diag::new(
                        Code::E114,
                        span(t.start, end),
                        format!("{{{text}}} after an edge is not a quantifier"),
                    );
                    return self.fail_d(d);
                }
                // P6: the `{` that starts no well-formed quantifier is the E001, with the generic text: the forms a
                // quantifier takes were expected, the `{` was found.
                const FORMS: [&str; 4] = ["{m,n}", "{m,}", "{m}", "{,n}"];
                let mut d = Diag::new(
                    Code::E001,
                    t.span(),
                    format!(
                        "expected {}, found `{{`",
                        FORMS.map(|f| format!("`{f}`")).join(", ")
                    ),
                );
                d.expected = FORMS.iter().map(|f| f.to_string()).collect();
                self.fail_d(d)
            }
        }
    }

    /// `{m,n}`, `{m,}`, `{m}` or `{,n}` ([LQ/grammar-v1.ebnf §P.6]). Its E114 cases (a bound above 4294967295,
    /// [LQ/lexical §8]; m > n) are located at the `{`: every E114 is located at the first token of the quantifier the
    /// rule refuses, as Annex R's detection point `edge_pat, quantifier` names it, whether the refusal is its form,
    /// its bounds or its being a second quantifier.
    fn brace_quant(&mut self) -> P<Quant> {
        let open = self.expect_punct(Punct::LBrace)?;
        let bound = |p: &mut Self| -> P<u32> {
            let t = p.la(0)?;
            match t.kind {
                TokKind::Int(n) if n > u32::MAX as i64 => {
                    let d = Diag::new(
                        Code::E114,
                        span(open.start, t.end),
                        format!("quantifier bound {n} is above 4294967295"),
                    );
                    p.fail_d(d)
                }
                TokKind::Int(n) => {
                    p.bump()?;
                    Ok(n as u32)
                }
                _ => p.unexpected(&["integer"]),
            }
        };
        let q = if self.is_punct(0, Punct::Comma)? {
            self.bump()?;
            let n = bound(self)?;
            Quant {
                min: 0,
                max: Some(n),
            }
        } else {
            let m = bound(self)?;
            if self.is_punct(0, Punct::Comma)? {
                self.bump()?;
                if self.is_punct(0, Punct::RBrace)? {
                    Quant { min: m, max: None }
                } else {
                    let n = bound(self)?;
                    Quant {
                        min: m,
                        max: Some(n),
                    }
                }
            } else {
                Quant {
                    min: m,
                    max: Some(m),
                }
            }
        };
        self.expect_punct(Punct::RBrace)?;
        if let Some(mx) = q.max
            && q.min > mx
        {
            let d = Diag::new(
                Code::E114,
                span(open.start, self.last_end),
                format!("quantifier {{{},{mx}}} has m > n", q.min),
            );
            return self.fail_d(d);
        }
        Ok(q)
    }

    // ----- expressions ---------------------------------------------------------------------------------------------

    fn expr(&mut self) -> P<Expr> {
        let mut l = self.and_expr()?;
        loop {
            if self.is_kw(0, "OR")? {
                self.bump_kw()?;
                let r = self.and_expr()?;
                let sp = l.span.to(r.span);
                l = Expr::new(ExprKind::Or(Box::new(l), Box::new(r)), sp);
            } else if self.is_kw(0, "XOR")? {
                return Err(self.xor_refusal(&l));
            } else {
                return Ok(l);
            }
        }
    }

    fn and_expr(&mut self) -> P<Expr> {
        let mut l = self.not_expr()?;
        while self.is_kw(0, "AND")? {
            self.bump_kw()?;
            let r = self.not_expr()?;
            let sp = l.span.to(r.span);
            l = Expr::new(ExprKind::And(Box::new(l), Box::new(r)), sp);
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> P<Expr> {
        if self.is_kw(0, "NOT")? {
            let t = self.la(0)?;
            self.enter(t.span())?;
            self.bump_kw()?;
            let e = self.not_expr()?;
            self.leave();
            let sp = t.span().to(e.span);
            return Ok(Expr::new(ExprKind::Not(Box::new(e)), sp));
        }
        self.pred_expr()
    }

    fn pred_expr(&mut self) -> P<Expr> {
        let l = self.add_expr()?;
        let t = self.la(0)?;
        match t.kind {
            TokKind::Punct(
                Punct::Eq
                | Punct::Ne
                | Punct::BangEq
                | Punct::Lt
                | Punct::Le
                | Punct::Gt
                | Punct::Ge
                | Punct::EqTilde,
            ) => self.comparison(l, &t),
            TokKind::Punct(Punct::Colon) => {
                self.bump()?;
                let labels = self.label_expr()?;
                let sp = span(l.span.start as usize, self.last_end);
                Ok(Expr::new(ExprKind::LabelTest(Box::new(l), labels), sp))
            }
            TokKind::Word => self.word_predicate(l, &t),
            _ => Ok(l),
        }
    }

    /// `add_expr cmp_op add_expr` with the E118 check of P18.
    fn comparison(&mut self, l: Expr, t: &Token) -> P<Expr> {
        let op = match t.kind {
            TokKind::Punct(Punct::Eq) => CmpOp::Eq,
            TokKind::Punct(Punct::Ne) => CmpOp::Ne,
            TokKind::Punct(Punct::BangEq) if self.strict => {
                return Err(self.strict_refusal(t.span(), "!=", "<>"));
            }
            TokKind::Punct(Punct::BangEq) => CmpOp::Ne,
            TokKind::Punct(Punct::Lt) => CmpOp::Lt,
            TokKind::Punct(Punct::Le) => CmpOp::Le,
            TokKind::Punct(Punct::Gt) => CmpOp::Gt,
            TokKind::Punct(Punct::Ge) => CmpOp::Ge,
            _ => {
                return Err(self.refusal(
                    t.span(),
                    "=~",
                    "use CONTAINS, STARTS WITH, glob_match() or search()",
                ));
            }
        };
        self.bump()?;
        let r = self.add_expr()?;
        if matches!(op, CmpOp::Eq | CmpOp::Ne)
            && (l.kind == ExprKind::Null || r.kind == ExprKind::Null)
        {
            return Err(self.null_comparison(op, t.span(), &l, &r));
        }
        let sp = l.span.to(r.span);
        Ok(Expr::new(ExprKind::Cmp(op, Box::new(l), Box::new(r)), sp))
    }

    /// E118 ([LQ/grammar-v1.ebnf §P.18]) at the comparison operator, as [50 §2.9]'s example places it (`1:33` of
    /// `MATCH (t:task) WHERE t.assignee = null RETURN t`).
    #[cold]
    #[inline(never)]
    fn null_comparison(&mut self, op: CmpOp, at: Span, l: &Expr, r: &Expr) -> Fail {
        let other = if l.kind == ExprKind::Null { r } else { l };
        let x = printer::expr_text(other);
        let inline = if op == CmpOp::Eq {
            format!("write {x} IS NULL")
        } else {
            format!("write {x} IS NOT NULL")
        };
        let d = Diag::new(Code::E118, at, "a comparison with NULL is never true").inline(inline);
        self.fail_now(d)
    }

    /// The word-led predicates: `IS [NOT] NULL`, `[NOT] IN`, `STARTS WITH`, `ENDS WITH`, `CONTAINS`.
    fn word_predicate(&mut self, l: Expr, t: &Token) -> P<Expr> {
        let w = self.text(t);
        if w.eq_ignore_ascii_case("IS") {
            self.bump_kw()?;
            if self.is_kw(0, "LABELED")? {
                return Err(self.labeled_refusal(&l));
            }
            let neg = if self.is_kw(0, "NOT")? {
                self.bump_kw()?;
                true
            } else {
                false
            };
            self.expect_kw("NULL")?;
            let sp = span(l.span.start as usize, self.last_end);
            return Ok(Expr::new(ExprKind::IsNull(neg, Box::new(l)), sp));
        }
        let (kind, words): (u8, usize) = if w.eq_ignore_ascii_case("NOT") && self.is_kw(1, "IN")? {
            (0, 2)
        } else if w.eq_ignore_ascii_case("IN") {
            (1, 1)
        } else if w.eq_ignore_ascii_case("STARTS") {
            (2, 1)
        } else if w.eq_ignore_ascii_case("ENDS") {
            (3, 1)
        } else if w.eq_ignore_ascii_case("CONTAINS") {
            (4, 1)
        } else {
            return Ok(l);
        };
        for _ in 0..words {
            self.bump_kw()?;
        }
        if kind == 2 || kind == 3 {
            self.expect_kw("WITH")?;
        }
        let r = self.add_expr()?;
        let sp = l.span.to(r.span);
        let (l, r) = (Box::new(l), Box::new(r));
        Ok(match kind {
            0 => Expr::new(
                ExprKind::Not(Box::new(Expr::new(ExprKind::In(l, r), sp))),
                sp,
            ),
            1 => Expr::new(ExprKind::In(l, r), sp),
            2 => Expr::new(ExprKind::StrPred(StrOp::Starts, l, r), sp),
            3 => Expr::new(ExprKind::StrPred(StrOp::Ends, l, r), sp),
            _ => Expr::new(ExprKind::StrPred(StrOp::Contains, l, r), sp),
        })
    }

    /// `x IS LABELED k` (Annex R: detected at `pred_expr: IS followed by LABELED`), E004 at the `LABELED` that makes
    /// the `IS` a label test.
    #[cold]
    #[inline(never)]
    fn labeled_refusal(&mut self, l: &Expr) -> Fail {
        let lt = match self.la(0) {
            Ok(t) => t,
            Err(f) => return f,
        };
        let k = self.peek_parse(|p| {
            p.consume()?;
            p.plain_name()
        });
        let inline = format!(
            "write {}:{}",
            self.source_of(l.span),
            k.map(|n| n.text).unwrap_or("<k>".into())
        );
        self.refusal(lt.span(), "x IS LABELED k", &inline)
    }

    #[cold]
    #[inline(never)]
    fn xor_refusal(&mut self, l: &Expr) -> Fail {
        let t = match self.la(0) {
            Ok(t) => t,
            Err(f) => return f,
        };
        let rhs = self.peek_parse(|p| {
            p.consume()?;
            p.and_expr()
        });
        let (a, b) = (
            self.source_of(l.span),
            rhs.map(|r| self.source_of(r.span))
                .unwrap_or_else(|| "<b>".into()),
        );
        self.refusal(
            t.span(),
            "a XOR b",
            &format!("write ({a} OR {b}) AND NOT ({a} AND {b})"),
        )
    }

    #[cold]
    #[inline(never)]
    fn modulo_refusal(&mut self, l: &Expr) -> Fail {
        let t = match self.la(0) {
            Ok(t) => t,
            Err(f) => return f,
        };
        let rhs = self.peek_parse(|p| {
            p.consume()?;
            p.unary_expr()
        });
        let (a, b) = (
            self.source_of(l.span),
            rhs.map(|r| self.source_of(r.span))
                .unwrap_or_else(|| "<b>".into()),
        );
        self.refusal(
            t.span(),
            "a % b",
            &format!("write {a} - {b} * toInteger({a} / {b})"),
        )
    }

    /// `x.f(args)` (Annex R: detected at the `(` after postfix `.` ident), E004 at that `(`. The rewrite is
    /// mechanical ([LQ/errors §6]): `write <f>(<x>, <args>)` with the arguments as written, read ahead without consuming
    /// them; with no arguments `write <f>(<x>)`, and `<args>` when they do not parse.
    #[cold]
    #[inline(never)]
    fn method_refusal(&mut self, name: &Name, e: &Expr) -> Fail {
        let open = match self.la(0) {
            Ok(t) => t,
            Err(f) => return f,
        };
        let args = self.peek_parse(|p| {
            let from = p.bump()?.end;
            p.args(None)?;
            Ok(p.src[from..p.last_end - 1].trim().to_string())
        });
        let x = self.source_of(e.span);
        let inline = match args {
            Some(a) if a.is_empty() => format!("write {}({x})", name.text),
            Some(a) => format!("write {}({x}, {a})", name.text),
            None => format!("write {}({x}, <args>)", name.text),
        };
        self.refusal(open.span(), "x.f(args)", &inline)
    }

    /// Records a diagnostic (the first of the pass wins) and returns the failure marker.
    fn fail_now(&mut self, d: Diag) -> Fail {
        if self.err.is_none() {
            self.err = Some(d);
        }
        Fail
    }

    /// E004 as a [`Fail`] (the cold form of [`Self::e004`]).
    #[cold]
    #[inline(never)]
    fn refusal(&mut self, at: Span, form: &str, inline: &str) -> Fail {
        let d = Diag::new(Code::E004, at, format!("{} is not in LQ", q(form))).inline(inline);
        self.fail_now(d)
    }

    #[cold]
    #[inline(never)]
    fn strict_refusal(&mut self, at: Span, form: &str, gql: &str) -> Fail {
        let d = Diag::new(
            Code::E004,
            at,
            format!(
                "{} is Cypher spelling; this surface takes the GQL spelling",
                q(form)
            ),
        )
        .inline(format!("write {gql}"));
        self.fail_now(d)
    }

    fn add_expr(&mut self) -> P<Expr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = if self.is_punct(0, Punct::Plus)? {
                ArithOp::Add
            } else if self.is_punct(0, Punct::Minus)? {
                ArithOp::Sub
            } else {
                return Ok(l);
            };
            self.bump()?;
            let r = self.mul_expr()?;
            let sp = l.span.to(r.span);
            l = Expr::new(ExprKind::Arith(op, Box::new(l), Box::new(r)), sp);
        }
    }

    fn mul_expr(&mut self) -> P<Expr> {
        let mut l = self.unary_expr()?;
        loop {
            let op = if self.is_punct(0, Punct::Star)? {
                ArithOp::Mul
            } else if self.is_punct(0, Punct::Slash)? {
                ArithOp::Div
            } else if self.is_punct(0, Punct::Percent)? {
                return Err(self.modulo_refusal(&l));
            } else {
                return Ok(l);
            };
            self.bump()?;
            let r = self.unary_expr()?;
            let sp = l.span.to(r.span);
            l = Expr::new(ExprKind::Arith(op, Box::new(l), Box::new(r)), sp);
        }
    }

    fn unary_expr(&mut self) -> P<Expr> {
        if self.is_punct(0, Punct::Minus)? {
            let t = self.bump()?;
            let e = self.postfix_expr()?;
            let sp = t.span().to(e.span);
            return Ok(Expr::new(ExprKind::Neg(Box::new(e)), sp));
        }
        self.postfix_expr()
    }

    fn postfix_expr(&mut self) -> P<Expr> {
        let mut e = self.primary()?;
        while self.is_punct(0, Punct::Dot)? {
            self.bump()?;
            let name = self.plain_name()?;
            if self.is_punct(0, Punct::LParen)? {
                return Err(self.method_refusal(&name, &e));
            }
            let sp = span(e.span.start as usize, self.last_end);
            e = Expr::new(ExprKind::Prop(Box::new(e), name), sp);
        }
        if self.is_punct(0, Punct::LBracket)? {
            let t = self.la(0)?;
            return Err(self.refusal(t.span(), "x[i]", "use IN or any(), all(), none()"));
        }
        Ok(e)
    }

    fn primary(&mut self) -> P<Expr> {
        let t = self.la(0)?;
        match &t.kind {
            TokKind::Int(_) | TokKind::Float | TokKind::Dur(_) | TokKind::Str | TokKind::Param => {
                self.literal(t)
            }
            TokKind::Node(_) | TokKind::Uid => self.node_literal(),
            TokKind::Punct(Punct::Colon) if self.is_word(1)? => Err(self.colon_param(&t)),
            TokKind::Punct(Punct::LParen) => self.paren_or_path(),
            TokKind::Punct(Punct::LBracket) => self.list_lit(),
            TokKind::Punct(Punct::LBrace) => self.map_lit(),
            TokKind::QIdent if self.is_punct(1, Punct::LParen)? => {
                let name = self.plain_name()?;
                self.generic_call(name, false)
            }
            TokKind::QIdent => Ok(Expr::new(ExprKind::Ident(self.bump_decoded()?), t.span())),
            TokKind::Word => self.word_primary(t),
            _ => self.unexpected(&["expression"]),
        }
    }

    fn literal(&mut self, t: Token) -> P<Expr> {
        let sp = t.span();
        let kind = match t.kind {
            TokKind::Int(n) => ExprKind::Int(n),
            TokKind::Float => ExprKind::Float(self.text(&t).to_string()),
            TokKind::Dur(_) => ExprKind::Dur(self.text(&t).to_string()),
            TokKind::Str => return Ok(Expr::new(ExprKind::Str(self.bump_decoded()?), sp)),
            _ => ExprKind::Param(self.src[t.start + 1..t.end].to_string()),
        };
        self.bump()?;
        Ok(Expr::new(kind, sp))
    }

    fn map_lit(&mut self) -> P<Expr> {
        let start = self.la(0)?.start;
        let entries = self.prop_map(false, None)?;
        Ok(Expr::new(
            ExprKind::Map(entries),
            span(start, self.last_end),
        ))
    }

    #[cold]
    #[inline(never)]
    fn colon_param(&mut self, t: &Token) -> Fail {
        let n = match self.la(1) {
            Ok(n) => n,
            Err(f) => return f,
        };
        let name = self.text(&n).to_string();
        let d = Diag::new(
            Code::E001,
            t.span().to(n.span()),
            format!(":{name} is not a parameter"),
        )
        .inline(format!("write ${name}"));
        self.fail_now(d)
    }

    fn word_primary(&mut self, t: Token) -> P<Expr> {
        let sp = t.span();
        let w = self.text(&t);
        if w.eq_ignore_ascii_case("TRUE") || w.eq_ignore_ascii_case("FALSE") {
            let b = w.eq_ignore_ascii_case("TRUE");
            self.bump_kw()?;
            return Ok(Expr::new(ExprKind::Bool(b), sp));
        }
        if w.eq_ignore_ascii_case("NULL") {
            self.bump_kw()?;
            return Ok(Expr::new(ExprKind::Null, sp));
        }
        if w.eq_ignore_ascii_case("CASE") {
            return self.case_expr();
        }
        let exists = w.eq_ignore_ascii_case("EXISTS");
        if (exists || w.eq_ignore_ascii_case("COUNT")) && self.is_punct(1, Punct::LBrace)? {
            self.bump_kw()?;
            let sub = Box::new(self.braced_subquery()?);
            let sp = span(t.start, self.last_end);
            return Ok(Expr::new(
                if exists {
                    ExprKind::Exists(sub)
                } else {
                    ExprKind::CountSub(sub)
                },
                sp,
            ));
        }
        if is_reserved(w) && !exists {
            return self.unexpected(&["expression"]);
        }
        if self.is_punct(1, Punct::LParen)? {
            return self.func_call();
        }
        if exists {
            return self.unexpected(&["expression"]);
        }
        let e = Expr::new(ExprKind::Ident(w.to_string()), sp);
        self.bump()?;
        Ok(e)
    }

    /// The refused function names at a function position ([LQ/grammar-v1.ebnf §R]).
    fn refused_function(&mut self, t: &Token) -> P<()> {
        let w = self.text(t).to_ascii_lowercase();
        let name = self.text(t).to_string();
        match w.as_str() {
            "shortestpath" | "allshortestpaths" | "nodes" | "relationships" => self.e004(
                t.span(),
                &format!("{name}("),
                "write CALL blockers(#N, transitive: true) YIELD blocker, depth, via",
            ),
            "single" => self.e004(
                t.span(),
                "single(x IN l WHERE p)",
                "write COUNT { ... } = 1",
            ),
            "timestamp" => self.e004(t.span(), "timestamp()", "write now()"),
            "cast" => {
                let inner = self.peek_parse(|p| {
                    p.consume()?;
                    p.consume()?;
                    p.expr()
                });
                let x = inner
                    .map(|e| self.source_of(e.span))
                    .unwrap_or_else(|| "<x>".into());
                self.e004(
                    t.span(),
                    "CAST(x AS t)",
                    &format!("write toInteger({x}), toFloat({x}) or toString({x})"),
                )
            }
            _ => Ok(()),
        }
    }

    fn func_call(&mut self) -> P<Expr> {
        let t = self.la(0)?;
        self.refused_function(&t)?;
        let w = self.text(&t);
        if w.eq_ignore_ascii_case("count")
            && self.is_punct(2, Punct::Star)?
            && self.is_punct(3, Punct::RParen)?
        {
            return self.count_star(&t);
        }
        if w.eq_ignore_ascii_case("all")
            || w.eq_ignore_ascii_case("any")
            || w.eq_ignore_ascii_case("none")
        {
            return self.list_pred(&t);
        }
        if (w.eq_ignore_ascii_case("exists") || w.eq_ignore_ascii_case("size"))
            && self.is_punct(2, Punct::LParen)?
            && let Some(e) = self.path_function(&t)?
        {
            return Ok(e);
        }
        // `EXISTS` is reserved ([LQ/lexical §6.1]), so it is no `ident`: `exists( e )` is its keyword form
        // ([LQ/grammar-v1.ebnf §P.9]) and the token stream prints it `KW` ([LQ/lexical §11]).
        let name = if is_reserved(w) {
            let name = Name::new(w, t.span());
            self.bump_kw()?;
            name
        } else {
            self.plain_name()?
        };
        self.generic_call(name, true)
    }

    fn count_star(&mut self, t: &Token) -> P<Expr> {
        let open = self.la(1)?;
        self.enter(open.span())?;
        self.bump_kw()?;
        self.bump()?;
        self.bump()?;
        self.bump()?;
        self.leave();
        Ok(Expr::new(ExprKind::CountStar, span(t.start, self.last_end)))
    }

    /// `all|any|none ( ident IN expr WHERE expr )` (P9: always the list predicate).
    fn list_pred(&mut self, t: &Token) -> P<Expr> {
        let kind = match self.text(t).to_ascii_lowercase().as_str() {
            "all" => ListPredKind::All,
            "any" => ListPredKind::Any,
            _ => ListPredKind::None,
        };
        let open = self.la(1)?;
        self.enter(open.span())?;
        self.bump_kw()?;
        self.bump()?;
        let var = self.var_name()?;
        if !self.is_kw(0, "IN")? {
            return self.unexpected(&["IN"]);
        }
        self.bump_kw()?;
        let list = Box::new(self.expr()?);
        if !self.is_kw(0, "WHERE")? {
            return self.unexpected(&["WHERE"]);
        }
        self.bump_kw()?;
        let pred = Box::new(self.expr()?);
        self.expect_punct(Punct::RParen)?;
        self.leave();
        Ok(Expr::new(
            ExprKind::ListPred {
                kind,
                var,
                list,
                pred,
            },
            span(t.start, self.last_end),
        ))
    }

    /// `exists( path )` and `size( path )` (P9): the path is tried first; `None` restores the position for the
    /// generic call.
    fn path_function(&mut self, t: &Token) -> P<Option<Expr>> {
        let exists = self.text(t).eq_ignore_ascii_case("exists");
        let saved = self.save();
        let err = self.err.take();
        match self.path_function_attempt() {
            Ok(Some(path)) if !self.lexical => {
                self.err = err;
                let sp = span(t.start, self.last_end);
                if self.strict {
                    let (form, gql) = if exists {
                        ("exists(<path>)", "EXISTS { <path> }")
                    } else {
                        ("size(<path>)", "COUNT { <path> }")
                    };
                    return Err(self.strict_refusal(sp, form, gql));
                }
                let sub = Box::new(Sub::Patterns {
                    patterns: vec![path],
                    where_: None,
                });
                Ok(Some(Expr::new(
                    if exists {
                        ExprKind::Exists(sub)
                    } else {
                        ExprKind::CountSub(sub)
                    },
                    sp,
                )))
            }
            _ => {
                if self.lexical {
                    return Err(Fail);
                }
                self.restore(saved);
                self.err = err;
                Ok(None)
            }
        }
    }

    fn path_function_attempt(&mut self) -> P<Option<Path>> {
        let open = self.la(1)?;
        self.enter(open.span())?;
        self.bump_kw()?;
        self.bump()?;
        let Some(path) = self.try_path()? else {
            return Ok(None);
        };
        if !self.is_punct(0, Punct::RParen)? {
            return Ok(None);
        }
        self.bump()?;
        self.leave();
        Ok(Some(path))
    }

    /// `ident ( [DISTINCT] [args] )` with the normalisation of `exists(e)` ([LQ/canonical-ast §3.1] item 6) and the
    /// strict-mode function spellings. `word` is false for a back-quoted name (never a keyword form).
    fn generic_call(&mut self, name: Name, word: bool) -> P<Expr> {
        let open = self.la(0)?;
        self.enter(open.span())?;
        self.bump()?;
        let distinct = if self.is_kw(0, "DISTINCT")? {
            self.bump_kw()?;
            true
        } else {
            false
        };
        let args = self.args(None)?;
        self.leave();
        let sp = span(name.span.start as usize, self.last_end);
        self.call_spellings(name, word, distinct, args, sp)
    }

    /// The strict-mode function spellings ([LQ/grammar-v1.ebnf §G.2]: matched ASCII-case-insensitively, a back-quoted
    /// name included, since it names the same function) and, for an unquoted `exists`, the normalisation of
    /// `exists(e)` ([LQ/canonical-ast §3.1] item 6).
    fn call_spellings(
        &mut self,
        name: Name,
        word: bool,
        distinct: bool,
        mut args: Vec<Arg>,
        sp: Span,
    ) -> P<Expr> {
        let lw = name.text.to_ascii_lowercase();
        if self.strict {
            let gql = match lw.as_str() {
                "tolower" => Some("lower("),
                "toupper" => Some("upper("),
                "collect" => Some("collect_list("),
                "size" => Some("cardinality("),
                _ => None,
            };
            if let Some(g) = gql {
                return Err(self.strict_refusal(name.span, &format!("{}(", name.text), g));
            }
        }
        if word
            && lw == "exists"
            && !distinct
            && args.len() == 1
            && args[0].name.is_none()
            && matches!(args[0].value, ArgVal::Expr(_))
        {
            let Some(Arg {
                value: ArgVal::Expr(e),
                ..
            }) = args.pop()
            else {
                unreachable!("checked above")
            };
            if self.strict {
                let x = printer::expr_text(&e);
                return Err(self.strict_refusal(
                    sp,
                    &format!("exists({x})"),
                    &format!("{x} IS NOT NULL"),
                ));
            }
            return Ok(Expr::new(ExprKind::IsNull(true, Box::new(e)), sp));
        }
        Ok(Expr::new(
            ExprKind::Fn {
                name,
                distinct,
                args,
            },
            sp,
        ))
    }

    /// P5: a path in expression position, tried when the tokens after the first node pattern are an edge operator or
    /// `(`. Returns `None` (with the position restored by the caller) when the start condition does not hold. A first
    /// node pattern that fails is the path alternative failing: its error competes with the parenthesised expression's
    /// by how far each got ([`Self::choose_error`]).
    fn try_path(&mut self) -> P<Option<Path>> {
        let np = self.node_pat(true)?;
        let starts = self.is_punct(0, Punct::LArrow)?
            || (self.is_punct(0, Punct::Minus)?
                && (self.is_punct(1, Punct::LBracket)?
                    || self.is_punct(1, Punct::Minus)?
                    || self.is_punct(1, Punct::Arrow)?))
            || self.is_punct(0, Punct::LParen)?;
        if !starts {
            return Ok(None);
        }
        let path = self.path_rest(np)?;
        Ok(Some(path))
    }

    fn paren_or_path(&mut self) -> P<Expr> {
        let open = self.la(0)?;
        let saved = self.save();
        let outer_err = self.err.take();
        // The path alternative (path_pred).
        let path_err = match self.path_pred_attempt(&open) {
            Ok(Some(path)) => {
                self.err = outer_err;
                return self.path_pred(path);
            }
            Ok(None) => None,
            Err(Fail) => {
                if self.lexical {
                    return Err(Fail);
                }
                self.err.take()
            }
        };
        self.restore(saved);
        // The parenthesised expression.
        match self.paren_expr(&open) {
            Ok(e) => {
                self.err = outer_err;
                Ok(e)
            }
            Err(Fail) => {
                let paren_err = self.err.take();
                self.err = outer_err;
                Err(self.choose_error(path_err, paren_err, &open))
            }
        }
    }

    fn path_pred_attempt(&mut self, open: &Token) -> P<Option<Path>> {
        self.enter(open.span())?;
        let r = self.try_path()?;
        self.leave();
        Ok(r)
    }

    fn path_pred(&mut self, path: Path) -> P<Expr> {
        let sp = path.span;
        if self.strict {
            let text = printer::path_text(&path);
            return Err(self.strict_refusal(sp, &text, &format!("EXISTS {{ {text} }}")));
        }
        let sub = Sub::Patterns {
            patterns: vec![path],
            where_: None,
        };
        Ok(Expr::new(ExprKind::Exists(Box::new(sub)), sp))
    }

    fn paren_expr(&mut self, open: &Token) -> P<Expr> {
        self.enter(open.span())?;
        self.bump()?;
        let e = self.expr()?;
        self.expect_punct(Punct::RParen)?;
        self.leave();
        Ok(e)
    }

    /// P5: when both alternatives fail, the error of the one that got further; on a tie, the path's.
    #[cold]
    #[inline(never)]
    fn choose_error(
        &mut self,
        path_err: Option<Diag>,
        paren_err: Option<Diag>,
        open: &Token,
    ) -> Fail {
        let chosen = match (path_err, paren_err) {
            (Some(a), Some(b)) => {
                // On a tie the path's error, unless the parenthesised expression met the nesting limit there: both are
                // E001 at one byte, and P13's text names the limit ([LQ/grammar-v1.ebnf §P.13]).
                if b.start() > a.start() || (b.start() == a.start() && b.message == TOO_DEEP) {
                    b
                } else {
                    a
                }
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => Diag::new(Code::E001, open.span(), "syntax error"),
        };
        self.fail_now(chosen)
    }

    fn list_lit(&mut self) -> P<Expr> {
        let open = self.la(0)?;
        self.enter(open.span())?;
        self.bump()?;
        if self.is_word(0)? && self.is_kw(1, "IN")? {
            return self.e004(
                open.span(),
                "[x IN l WHERE p | e]",
                "use any(), all(), none(), IN, COUNT { } or UNWIND",
            );
        }
        let mut elems = Vec::new();
        if !self.is_punct(0, Punct::RBracket)? {
            loop {
                elems.push(self.expr()?);
                if self.is_punct(0, Punct::Pipe)? {
                    let t = self.la(0)?;
                    return self.e004(
                        t.span(),
                        "[... | ...]",
                        "use any(), all(), none(), IN, COUNT { } or UNWIND",
                    );
                }
                if self.is_punct(0, Punct::Comma)? {
                    self.bump()?;
                } else {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RBracket)?;
        self.leave();
        Ok(Expr::new(
            ExprKind::List(elems),
            span(open.start, self.last_end),
        ))
    }

    fn case_expr(&mut self) -> P<Expr> {
        let t = self.la(0)?;
        self.enter(t.span())?;
        self.bump_kw()?;
        let subject = if self.is_kw(0, "WHEN")? {
            None
        } else {
            Some(Box::new(self.expr()?))
        };
        let mut whens = Vec::new();
        if !self.is_kw(0, "WHEN")? {
            return self.unexpected(&["WHEN"]);
        }
        while self.is_kw(0, "WHEN")? {
            self.bump_kw()?;
            let cond = self.expr()?;
            self.expect_kw("THEN")?;
            let then = self.expr()?;
            whens.push(When { cond, then });
        }
        let else_ = if self.is_kw(0, "ELSE")? {
            self.bump_kw()?;
            Some(Box::new(self.expr()?))
        } else {
            None
        };
        self.expect_kw("END")?;
        self.leave();
        Ok(Expr::new(
            ExprKind::Case {
                subject,
                whens,
                else_,
            },
            span(t.start, self.last_end),
        ))
    }

    /// `{ subquery }` after `EXISTS`, `COUNT` or `UNLESS EXISTS` (P8).
    fn braced_subquery(&mut self) -> P<Sub> {
        let open = self.la(0)?;
        self.enter(open.span())?;
        self.bump()?;
        let t = self.la(0)?;
        let sub = if self.is_kw(0, "USE")? {
            return self.e308(t.span());
        } else if t.kind == TokKind::Punct(Punct::RBrace) {
            let d = Diag::new(
                Code::E001,
                t.span(),
                "EXISTS { } needs a pattern or clauses",
            );
            return self.fail_d(d);
        } else if t.kind == TokKind::Punct(Punct::LParen) {
            let patterns = self.pattern_list()?;
            let where_ = self.opt_where()?;
            Sub::Patterns { patterns, where_ }
        } else if self.is_kw_any(
            0,
            &["MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH", "RETURN"],
        )? {
            let mut clauses = Vec::new();
            loop {
                if self.is_kw(0, "USE")? {
                    let at = self.la(0)?.span();
                    return self.e308(at);
                }
                if self.is_kw(0, "RETURN")? || self.is_punct(0, Punct::RBrace)? {
                    break;
                }
                if self.is_kw(0, "MATCH")? || self.is_kw(0, "OPTIONAL")? {
                    clauses.push(Clause::Match(self.match_clause()?));
                } else if self.is_kw(0, "CALL")? && !self.is_punct(1, Punct::LBrace)? {
                    let head = self.call_head()?;
                    clauses.push(Clause::Call(self.call_clause_of(head)?));
                } else if self.is_kw(0, "UNWIND")? {
                    clauses.push(Clause::Unwind(self.unwind()?));
                } else if self.is_kw(0, "WITH")? {
                    clauses.push(Clause::With(self.with_clause()?));
                } else {
                    self.clause_position_refusals(true)?;
                    return self.unexpected(&[
                        "MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH", "RETURN", "}",
                    ]);
                }
            }
            let ret = if self.is_kw(0, "RETURN")? {
                Some(self.return_clause()?)
            } else {
                None
            };
            Sub::Clauses { clauses, ret }
        } else if t.kind == TokKind::Word
            && !is_reserved(self.text(&t))
            && self.is_punct(1, Punct::Eq)?
        {
            self.pattern_start()?;
            return self.unexpected(&["("]);
        } else {
            return self
                .unexpected(&["(", "MATCH", "OPTIONAL", "CALL", "UNWIND", "WITH", "RETURN"]);
        };
        self.expect_punct(Punct::RBrace)?;
        self.leave();
        Ok(sub)
    }

    fn e308<T>(&mut self, at: Span) -> P<T> {
        let d = Diag::new(
            Code::E308,
            at,
            "USE inside EXISTS {} or COUNT {} is not allowed",
        )
        .help("compare versions with diff(), across() or a composite query");
        self.fail_d(d)
    }

    // ----- transactions --------------------------------------------------------------------------------------------

    fn string_value(&mut self) -> P<String> {
        let t = self.la(0)?;
        match &t.kind {
            TokKind::Str => self.bump_decoded(),
            _ => self.unexpected(&["string"]),
        }
    }

    fn tx(&mut self) -> P<Tx> {
        let start = self.bump_kw()?.start;
        let mut tx = Tx {
            on: None,
            if_tip: None,
            if_targets: None,
            key: None,
            lease: None,
            message: None,
            stmts: Vec::new(),
            dry: false,
            span: Span::default(),
        };
        loop {
            let t = self.la(0)?;
            if t.kind != TokKind::Word {
                break;
            }
            let w = upper(self.text(&t));
            let once = |p: &mut Self, set: bool, what: &str| -> P<()> {
                if set {
                    let d = Diag::new(Code::E001, t.span(), format!("TX takes {what} once"));
                    return p.fail_d(d);
                }
                Ok(())
            };
            match w.as_str() {
                "ON" => {
                    once(self, tx.on.is_some(), "ON")?;
                    self.bump_kw()?;
                    tx.on = Some(self.revspec_here()?);
                }
                "IF" => {
                    if self.is_kw(1, "TIP")? {
                        once(self, tx.if_tip.is_some(), "IF TIP")?;
                        self.bump_kw()?;
                        self.bump_kw()?;
                        tx.if_tip = Some(self.revspec_here()?);
                    } else if self.is_kw(1, "TARGETS")? {
                        once(self, tx.if_targets.is_some(), "IF TARGETS")?;
                        self.bump_kw()?;
                        self.bump_kw()?;
                        tx.if_targets = Some(self.string_value()?);
                    } else {
                        self.bump_kw()?;
                        return self.unexpected(&["TIP", "TARGETS"]);
                    }
                }
                "KEY" => {
                    once(self, tx.key.is_some(), "KEY")?;
                    self.bump_kw()?;
                    tx.key = Some(self.string_value()?);
                }
                "LEASE" => {
                    once(self, tx.lease.is_some(), "LEASE")?;
                    self.bump_kw()?;
                    tx.lease = Some(self.string_value()?);
                }
                "MESSAGE" => {
                    once(self, tx.message.is_some(), "MESSAGE")?;
                    self.bump_kw()?;
                    tx.message = Some(self.string_value()?);
                }
                _ => break,
            }
        }
        if !self.is_punct(0, Punct::LBrace)? {
            return self.unexpected(&["{", "ON", "IF", "KEY", "LEASE", "MESSAGE"]);
        }
        self.bump()?;
        if self.is_punct(0, Punct::RBrace)? {
            let t = self.la(0)?;
            let d = Diag::new(Code::E009, t.span(), "TX block has no write statement")
                .help("read with moirai q or the query tool; TX is for writes");
            return self.fail_d(d);
        }
        'stmts: loop {
            let saved_depth = self.depth;
            match self.tx_stmt() {
                Ok(s) => tx.stmts.push(s),
                Err(Fail) => {
                    self.depth = saved_depth;
                    if !self.recover(&[";", "}"]) {
                        return Err(Fail);
                    }
                }
            }
            loop {
                if self.is_punct(0, Punct::Semi)? {
                    self.bump()?;
                    if self.is_punct(0, Punct::RBrace)? {
                        break 'stmts;
                    }
                    continue 'stmts;
                }
                if self.is_punct(0, Punct::RBrace)? {
                    break 'stmts;
                }
                self.unexpected::<()>(&[";", "}"]).ok();
                if !self.recover(&[";", "}"]) {
                    return Err(Fail);
                }
            }
        }
        self.bump()?; // '}'
        if self.is_kw(0, "DRY")? {
            self.bump_kw()?;
            tx.dry = true;
        }
        tx.span = span(start, self.last_end);
        Ok(tx)
    }

    fn is_mutation_start(&mut self) -> P<bool> {
        if self.is_kw_any(0, &["SET", "REMOVE", "DELETE", "MOVE", "REOPEN", "PATCH"])? {
            return Ok(true);
        }
        Ok(self.is_kw_any(0, &["CREATE", "INSERT"])?
            && self.is_punct(1, Punct::LParen)?
            && self.is_punct(3, Punct::RParen)?)
    }

    fn tx_stmt(&mut self) -> P<Stmt> {
        let t = self.la(0)?;
        if t.kind != TokKind::Word {
            return self.unexpected(&[
                "MATCH", "SET", "CREATE", "CALL", "ASSERT", "RESOLVE", "DEFINE", "DROP",
            ]);
        }
        let w = upper(self.text(&t));
        self.clause_position_refusals(false)?;
        match w.as_str() {
            "MATCH" => {
                self.bump_kw()?;
                let patterns = self.pattern_list()?;
                let where_ = self.opt_where()?;
                if !self.is_kw(0, "EXPECT")? {
                    let at = self.la(0)?.span();
                    let d = Diag::new(Code::E007, at, "MATCH in TX needs EXPECT")
                        .inline("add EXPECT <n>, a range a..b, <= n or >= n")
                        .help("EXPECT turns \"matched nothing\" into a guard failure");
                    return self.fail_d(d);
                }
                self.bump_kw()?;
                let expect = self.expect()?;
                let muts = self.mutations()?;
                Ok(Stmt::Match(SMatch {
                    patterns,
                    where_,
                    expect,
                    muts,
                    span: span(t.start, self.last_end),
                }))
            }
            "CREATE" | "INSERT"
                if self.is_punct(1, Punct::LParen)? && self.is_punct(3, Punct::Colon)? =>
            {
                Ok(Stmt::Create(self.create_stmt()?))
            }
            "SET" | "REMOVE" | "DELETE" | "MOVE" | "REOPEN" | "PATCH" | "CREATE" | "INSERT" => {
                let muts = self.mutations()?;
                Ok(Stmt::Muts(muts, span(t.start, self.last_end)))
            }
            "CALL" => {
                self.bump_kw()?;
                let name = self.proc_name(true)?;
                let short = match name.text.split_once('.') {
                    Some((first, rest))
                        if first.eq_ignore_ascii_case("tx") && !rest.contains('.') =>
                    {
                        rest.to_string()
                    }
                    _ => {
                        let d = Diag::new(Code::E109, name.span, "inside TX, CALL names a named mutation")
                            .help("CALL queries() lists the named queries; the built-ins are in reference-ql.md");
                        return self.fail_d(d);
                    }
                };
                self.expect_punct(Punct::LParen)?;
                let args = self.args(None)?;
                let yield_ = if self.is_kw(0, "YIELD")? {
                    self.bump_kw()?;
                    self.yield_items()?
                } else {
                    Vec::new()
                };
                let nspan = Span::new(name.span.start + 3, name.span.end);
                Ok(Stmt::TxCall {
                    name: Name::new(short, nspan),
                    args,
                    yield_,
                    span: span(t.start, self.last_end),
                })
            }
            "ASSERT" => {
                self.bump_kw()?;
                let expr = self.expr()?;
                let else_ = if self.is_kw(0, "ELSE")? {
                    self.bump_kw()?;
                    Some(self.string_value()?)
                } else {
                    None
                };
                Ok(Stmt::Assert {
                    expr,
                    else_,
                    span: span(t.start, self.last_end),
                })
            }
            "RESOLVE" => Ok(Stmt::Resolve(self.resolve()?)),
            "DEFINE" => Ok(Stmt::Define(self.define()?)),
            "DROP" => {
                self.bump_kw()?;
                self.expect_kw("QUERY")?;
                Ok(Stmt::Drop(self.qname()?))
            }
            _ => self.unexpected(&[
                "MATCH", "SET", "CREATE", "CALL", "ASSERT", "RESOLVE", "DEFINE", "DROP",
            ]),
        }
    }

    fn expect(&mut self) -> P<Expect> {
        let t = self.la(0)?;
        let int = |p: &mut Self| -> P<i64> {
            match p.la(0)?.kind {
                TokKind::Int(n) => {
                    p.bump()?;
                    Ok(n)
                }
                _ => p.unexpected(&["integer"]),
            }
        };
        match t.kind {
            TokKind::Int(n) => {
                self.bump()?;
                if self.is_punct(0, Punct::DotDot)? {
                    self.bump()?;
                    let m = int(self)?;
                    Ok(Expect::Range(n, m))
                } else {
                    Ok(Expect::Exact(n))
                }
            }
            TokKind::Punct(Punct::Le) => {
                self.bump()?;
                Ok(Expect::Le(int(self)?))
            }
            TokKind::Punct(Punct::Ge) => {
                self.bump()?;
                Ok(Expect::Ge(int(self)?))
            }
            TokKind::Param => {
                self.bump()?;
                Ok(Expect::Param(Name::new(
                    &self.src[t.start + 1..t.end],
                    t.span(),
                )))
            }
            _ => self.unexpected(&["integer", "<=", ">=", "$param"]),
        }
    }

    fn target(&mut self) -> P<Target> {
        let t = self.la(0)?;
        let kind = match &t.kind {
            TokKind::Node(n) => TargetKind::Nid(*n),
            TokKind::Uid => TargetKind::Uid(self.src[t.start + 3..t.end].to_string()),
            TokKind::Param => TargetKind::Param(self.src[t.start + 1..t.end].to_string()),
            TokKind::QIdent => TargetKind::Ident(self.lx.qident_value(&t)),
            TokKind::Word if !is_reserved(self.text(&t)) => {
                TargetKind::Ident(self.text(&t).to_string())
            }
            _ => return self.unexpected(&["name", "#N", "$param"]),
        };
        self.bump()?;
        Ok(Target {
            kind,
            span: t.span(),
        })
    }

    /// One or more mutations; node creation inside the list is E001 (P11).
    fn mutations(&mut self) -> P<Vec<Mut>> {
        let mut muts = vec![self.mutation()?];
        loop {
            if self.is_kw_any(0, &["DETACH", "NODETACH"])? {
                self.clause_position_refusals(false)?;
            }
            if self.is_kw_any(0, &["CREATE", "INSERT"])?
                && self.is_punct(1, Punct::LParen)?
                && self.is_punct(3, Punct::Colon)?
            {
                let at = self.la(0)?.span();
                let d = Diag::new(Code::E001, at, "node creation is a statement of its own")
                    .inline("end the MATCH statement with ; before CREATE");
                return self.fail_d(d);
            }
            if !self.is_mutation_start()? {
                return Ok(muts);
            }
            muts.push(self.mutation()?);
        }
    }

    fn mutation(&mut self) -> P<Mut> {
        self.clause_position_refusals(false)?;
        let t = self.la(0)?;
        if t.kind != TokKind::Word {
            return self.unexpected(&[
                "SET", "REMOVE", "DELETE", "MOVE", "CREATE", "REOPEN", "PATCH",
            ]);
        }
        let w = upper(self.text(&t));
        match w.as_str() {
            "SET" => {
                self.bump_kw()?;
                let mut assigns = Vec::new();
                loop {
                    let target = self.target()?;
                    self.expect_punct(Punct::Dot)?;
                    let prop = self.plain_name()?;
                    self.expect_punct(Punct::Eq)?;
                    let value = self.expr()?;
                    assigns.push(Assign {
                        target,
                        prop,
                        value,
                    });
                    if self.is_punct(0, Punct::Comma)? {
                        self.bump()?;
                    } else {
                        break;
                    }
                }
                Ok(Mut::Set(assigns, span(t.start, self.last_end)))
            }
            "REMOVE" => {
                self.bump_kw()?;
                let mut items = Vec::new();
                loop {
                    let target = self.target()?;
                    self.expect_punct(Punct::Dot)?;
                    let prop = self.plain_name()?;
                    items.push(TProp { target, prop });
                    if self.is_punct(0, Punct::Comma)? {
                        self.bump()?;
                    } else {
                        break;
                    }
                }
                Ok(Mut::Remove(items, span(t.start, self.last_end)))
            }
            "DELETE" => {
                self.bump_kw()?;
                let mut targets = vec![self.target()?];
                while self.is_punct(0, Punct::Comma)? {
                    self.bump()?;
                    targets.push(self.target()?);
                }
                let mut opts: Vec<DOpt> = Vec::new();
                loop {
                    let o = self.la(0)?;
                    if o.kind != TokKind::Word {
                        break;
                    }
                    let ow = upper(self.text(&o));
                    let (opt, what) = match ow.as_str() {
                        "POLICY" => {
                            self.bump_kw()?;
                            let v = if self.is_kw(0, "RESTRICT")? {
                                Policy::Restrict
                            } else if self.is_kw(0, "CASCADE")? {
                                Policy::Cascade
                            } else if self.is_kw(0, "REPARENT")? {
                                Policy::Reparent
                            } else {
                                return self.unexpected(&["RESTRICT", "CASCADE", "REPARENT"]);
                            };
                            self.bump_kw()?;
                            (DOpt::Policy(v), "POLICY")
                        }
                        "REPLACED" => {
                            self.bump_kw()?;
                            self.expect_kw("BY")?;
                            (DOpt::Replaced(self.target()?), "REPLACED BY")
                        }
                        "RELEASE" => {
                            self.bump_kw()?;
                            (DOpt::Release, "RELEASE")
                        }
                        "REASON" => {
                            self.bump_kw()?;
                            (DOpt::Reason(self.expr()?), "REASON")
                        }
                        _ => break,
                    };
                    let same =
                        |a: &DOpt, b: &DOpt| std::mem::discriminant(a) == std::mem::discriminant(b);
                    if opts.iter().any(|x| same(x, &opt)) {
                        let d =
                            Diag::new(Code::E001, o.span(), format!("DELETE takes {what} once"));
                        return self.fail_d(d);
                    }
                    opts.push(opt);
                }
                Ok(Mut::Delete {
                    targets,
                    opts,
                    span: span(t.start, self.last_end),
                })
            }
            "MOVE" => {
                self.bump_kw()?;
                let target = self.target()?;
                self.expect_kw("UNDER")?;
                let under = self.target()?;
                let pos = if self.is_kw(0, "BEFORE")? {
                    self.bump_kw()?;
                    Some(MovePos::Before(self.target()?))
                } else if self.is_kw(0, "AFTER")? {
                    self.bump_kw()?;
                    Some(MovePos::After(self.target()?))
                } else if self.is_kw(0, "FIRST")? {
                    self.bump_kw()?;
                    Some(MovePos::First)
                } else if self.is_kw(0, "LAST")? {
                    self.bump_kw()?;
                    Some(MovePos::Last)
                } else {
                    None
                };
                Ok(Mut::Move {
                    target,
                    under,
                    pos,
                    span: span(t.start, self.last_end),
                })
            }
            "CREATE" | "INSERT" => {
                if w == "CREATE" && self.strict {
                    return self.strict_refuse(t.span(), "CREATE", "INSERT");
                }
                self.bump_kw()?;
                self.expect_punct(Punct::LParen)?;
                let src = self.target()?;
                self.expect_punct(Punct::RParen)?;
                let (dir, ty, props) = self.edge_step()?;
                self.expect_punct(Punct::LParen)?;
                let dst = self.target()?;
                self.expect_punct(Punct::RParen)?;
                Ok(Mut::Edge(MEdge {
                    src,
                    dir,
                    ty,
                    props,
                    dst,
                    span: span(t.start, self.last_end),
                }))
            }
            "REOPEN" => {
                self.bump_kw()?;
                let target = self.target()?;
                self.expect_kw("REASON")?;
                let reason = self.expr()?;
                Ok(Mut::Reopen {
                    target,
                    reason,
                    span: span(t.start, self.last_end),
                })
            }
            "PATCH" => {
                self.bump_kw()?;
                let target = self.target()?;
                self.expect_punct(Punct::Dot)?;
                let field = self.plain_name()?;
                self.expect_kw("REMOVE")?;
                let remove = self.expr()?;
                self.expect_kw("ADD")?;
                let add = self.expr()?;
                Ok(Mut::Patch {
                    target,
                    field,
                    remove,
                    add,
                    span: span(t.start, self.last_end),
                })
            }
            _ => self.unexpected(&[
                "SET", "REMOVE", "DELETE", "MOVE", "CREATE", "REOPEN", "PATCH",
            ]),
        }
    }

    /// `edge_step`: `-[:T {…}]->` or `<-[:T {…}]-`.
    fn edge_step(&mut self) -> P<(EdgeDir, Name, Vec<Kv>)> {
        let left = if self.is_punct(0, Punct::LArrow)? {
            self.bump()?;
            true
        } else {
            self.expect_punct(Punct::Minus)?;
            false
        };
        self.expect_punct(Punct::LBracket)?;
        self.expect_punct(Punct::Colon)?;
        let ty = self.plain_name()?;
        let props = if self.is_punct(0, Punct::LBrace)? {
            self.prop_map(false, None)?
        } else {
            Vec::new()
        };
        self.expect_punct(Punct::RBracket)?;
        if left {
            self.expect_punct(Punct::Minus)?;
            Ok((EdgeDir::Left, ty, props))
        } else {
            self.expect_punct(Punct::Arrow)?;
            Ok((EdgeDir::Right, ty, props))
        }
    }

    fn create_stmt(&mut self) -> P<Create> {
        let t = self.la(0)?;
        if self.strict && self.is_kw(0, "CREATE")? {
            return self.strict_refuse(t.span(), "CREATE", "INSERT");
        }
        self.bump_kw()?;
        self.expect_punct(Punct::LParen)?;
        let var = self.var_name()?;
        self.expect_punct(Punct::Colon)?;
        let label = self.plain_name()?;
        let props = if self.is_punct(0, Punct::LBrace)? {
            self.prop_map(false, None)?
        } else {
            Vec::new()
        };
        self.expect_punct(Punct::RParen)?;
        let mut edges = Vec::new();
        while self.is_punct(0, Punct::Minus)? || self.is_punct(0, Punct::LArrow)? {
            let (dir, ty, eprops) = self.edge_step()?;
            self.expect_punct(Punct::LParen)?;
            let target = self.target()?;
            self.expect_punct(Punct::RParen)?;
            edges.push(CEdge {
                dir,
                ty,
                props: eprops,
                target,
            });
        }
        let under = if self.is_kw(0, "UNDER")? {
            self.bump_kw()?;
            Some(self.target()?)
        } else {
            None
        };
        let unless = if self.is_kw(0, "UNLESS")? {
            self.bump_kw()?;
            self.expect_kw("EXISTS")?;
            if !self.is_punct(0, Punct::LBrace)? {
                return self.unexpected(&["{"]);
            }
            Some(self.braced_subquery()?)
        } else {
            None
        };
        Ok(Create {
            var,
            label,
            props,
            edges,
            under,
            unless,
            span: span(t.start, self.last_end),
        })
    }

    fn resolve(&mut self) -> P<Resolve> {
        let start = self.bump_kw()?.start;
        let what = if self.is_punct(0, Punct::LParen)? {
            self.bump()?;
            let query = self.query()?;
            self.expect_punct(Punct::RParen)?;
            self.expect_kw("EXPECT")?;
            let e = self.expect()?;
            ResolveWhat::Query(Box::new(query), e)
        } else {
            ResolveWhat::Key(self.string_value()?)
        };
        self.expect_kw("TAKE")?;
        let take = if self.is_kw(0, "OURS")? {
            self.bump_kw()?;
            Take::Ours
        } else if self.is_kw(0, "THEIRS")? {
            self.bump_kw()?;
            Take::Theirs
        } else if self.is_kw(0, "BASE")? {
            self.bump_kw()?;
            Take::Base
        } else if self.is_kw(0, "VALUE")? {
            self.bump_kw()?;
            Take::Value(self.expr()?)
        } else if self.is_kw(0, "REPOINT")? {
            self.bump_kw()?;
            Take::Repoint(self.target()?)
        } else {
            return self.unexpected(&["OURS", "THEIRS", "BASE", "VALUE", "REPOINT"]);
        };
        Ok(Resolve {
            what,
            take,
            span: span(start, self.last_end),
        })
    }

    fn qname(&mut self) -> P<Name> {
        let first = self.plain_name()?;
        let mut text = first.text;
        while self.is_punct(0, Punct::Dot)? {
            self.bump()?;
            text.push('.');
            text.push_str(&self.plain_name()?.text);
        }
        Ok(Name::new(
            text,
            span(first.span.start as usize, self.last_end),
        ))
    }

    fn define(&mut self) -> P<Define> {
        let start = self.bump_kw()?.start;
        self.expect_kw("QUERY")?;
        let name = self.qname()?;
        self.expect_punct(Punct::LParen)?;
        let mut params = Vec::new();
        if !self.is_punct(0, Punct::RParen)? {
            loop {
                params.push(self.param_decl()?);
                if self.is_punct(0, Punct::Comma)? {
                    self.bump()?;
                } else {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RParen)?;
        let shape = if self.is_kw(0, "SHAPE")? {
            self.bump_kw()?;
            Some(self.plain_name()?)
        } else {
            None
        };
        let budget = if self.is_kw(0, "BUDGET")? {
            self.bump_kw()?;
            Some(self.plain_name()?)
        } else {
            None
        };
        self.expect_kw("AS")?;
        self.expect_punct(Punct::LBrace)?;
        let body = self.query()?;
        self.expect_punct(Punct::RBrace)?;
        Ok(Define {
            name,
            params,
            shape,
            budget,
            body,
            span: span(start, self.last_end),
        })
    }

    fn param_decl(&mut self) -> P<PDecl> {
        let t = self.la(0)?;
        if t.kind != TokKind::Param {
            return self.unexpected(&["$param"]);
        }
        self.bump()?;
        let name = Name::new(&self.src[t.start + 1..t.end], t.span());
        self.expect_punct(Punct::Colon)?;
        let tname = self.plain_name()?;
        let arg = if self.is_punct(0, Punct::Lt)? {
            self.bump()?;
            let a = self.plain_name()?;
            self.expect_punct(Punct::Gt)?;
            Some(a)
        } else {
            None
        };
        let optional = if self.is_punct(0, Punct::Question)? {
            self.bump()?;
            true
        } else {
            false
        };
        let default = if self.is_punct(0, Punct::Eq)? {
            self.bump()?;
            let v = self.la(0)?;
            let sp = v.span();
            Some(match &v.kind {
                TokKind::Int(n) => {
                    self.bump()?;
                    Expr::new(ExprKind::Int(*n), sp)
                }
                TokKind::Float => {
                    self.bump()?;
                    Expr::new(ExprKind::Float(self.text(&v).to_string()), sp)
                }
                TokKind::Dur(_) => {
                    self.bump()?;
                    Expr::new(ExprKind::Dur(self.text(&v).to_string()), sp)
                }
                TokKind::Str => Expr::new(ExprKind::Str(self.bump_decoded()?), sp),
                TokKind::Node(_) | TokKind::Uid => self.node_literal()?,
                TokKind::Word
                    if matches!(upper(self.text(&v)).as_str(), "TRUE" | "FALSE" | "NULL") =>
                {
                    let w = upper(self.text(&v));
                    self.bump_kw()?;
                    match w.as_str() {
                        "TRUE" => Expr::new(ExprKind::Bool(true), sp),
                        "FALSE" => Expr::new(ExprKind::Bool(false), sp),
                        _ => Expr::new(ExprKind::Null, sp),
                    }
                }
                _ => return self.unexpected(&["literal"]),
            })
        } else {
            None
        };
        Ok(PDecl {
            name,
            ty: PType { name: tname, arg },
            optional,
            default,
        })
    }
}

/// The parts of an `edge_body`.
#[derive(Default)]
struct EdgeBody {
    var: Option<Name>,
    types: Vec<Name>,
    quant: Option<Quant>,
    props: Vec<Kv>,
    where_: Option<Expr>,
}

/// Renders a classified token stream in the fixture format of [LQ/lexical §11]: `kind SP value LF` per token.
pub fn token_stream_text(tokens: &[TokOut]) -> String {
    let mut out = String::new();
    for t in tokens {
        out.push_str(t.kind);
        out.push(' ');
        out.push_str(&t.value);
        out.push('\n');
    }
    out
}
