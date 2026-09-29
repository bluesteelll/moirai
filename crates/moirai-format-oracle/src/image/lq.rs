//! The part of LQ a named-query file's checks need ([F14 §7.2.2]): the normal-mode tokens of [LQ/lexical §3], §5 up to
//! `AS {` of a `define_stmt` ([LQ/grammar-v1.ebnf] `define_stmt`, `param_decl`, `type`, `qname`), the rendering of its
//! `param_decl` list ([F14 §7.2.3]) and the name's canonical spelling (§7.2.1), and the node-literal pre-check of
//! [LQ/lexical §10.2].
//!
//! The oracle is not an LQ parser or binder: the query body after `AS {` is checked for balanced braces only, and
//! portability condition 2 of [LQ/lexical §10.2] only by its implied byte-level pre-check (a node literal token refuses;
//! its absence decides nothing). The full parse and the re-binding are the reference model's and the engine's.

/// The head of a `define_stmt`: what [F14 §7.2.2]'s consistency rules compare with the file's lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefineHead {
    /// The `qname` in its canonical spelling ([F14 §7.2.1]).
    pub name: String,
    /// The rendered `param_decl` list ([F14 §7.2.3]); empty when the query has no parameter.
    pub params: String,
    /// The `SHAPE` word, as written.
    pub shape: Option<String>,
    /// The `BUDGET` word, as written.
    pub budget: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// A word ([LQ/lexical §5.3]).
    Word(String),
    /// A back-quoted identifier, decoded (§5.4).
    Quoted(String),
    /// `$` + word (§5.5): the word.
    Param(String),
    /// A number, duration, string, node or uid literal (§5.6–§5.8): its exact source text.
    Lit(String),
    /// Punctuation (§5.9).
    Punct(&'static str),
}

/// [LQ/lexical §5.9], two-byte tokens first (longest match).
const PUNCT: [&str; 28] = [
    "..", "->", "<-", "<>", "<=", ">=", "!=", "=~", "(", ")", "[", "]", "{", "}", ",", ";", ".",
    ":", "|", "+", "-", "*", "/", "=", "<", ">", "?", "%",
];

fn is_word_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// A normal-mode scanner over the text ([LQ/lexical §3], §5).
struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(t: &'a str) -> Self {
        Lexer {
            s: t.as_bytes(),
            i: 0,
        }
    }

    fn fail<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("byte {}: {m} [LQ/lexical]", self.i))
    }

    /// Skips whitespace and comments (§3).
    fn skip(&mut self) -> Result<(), String> {
        loop {
            match self.s.get(self.i..) {
                Some([b' ' | b'\t' | b'\n' | b'\r', ..]) => self.i += 1,
                Some([b'/', b'/', ..]) => {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                Some([b'/', b'*', ..]) => {
                    let start = self.i;
                    self.i += 2;
                    loop {
                        match self.s.get(self.i..) {
                            Some([b'*', b'/', ..]) => {
                                self.i += 2;
                                break;
                            }
                            Some([_, ..]) => self.i += 1,
                            _ => {
                                self.i = start;
                                return self.fail("an unclosed block comment (E002)");
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// A string or back-quoted identifier starting at the delimiter `q`; returns its source text and decoded value.
    fn delimited(&mut self, q: u8) -> Result<(String, String), String> {
        let start = self.i;
        self.i += 1;
        let mut val = String::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                self.i = start;
                return self.fail("an unterminated string or back-quoted name (E002)");
            };
            match c {
                b'\n' | b'\r' => {
                    self.i = start;
                    return self.fail("a line end inside a string or back-quoted name (E002)");
                }
                _ if c == q && q == b'`' && self.s.get(self.i + 1) == Some(&b'`') => {
                    val.push('`');
                    self.i += 2;
                }
                _ if c == q => {
                    self.i += 1;
                    break;
                }
                b'\\' if q != b'`' => {
                    let e = self.s.get(self.i + 1).copied();
                    let (ch, n) = match e {
                        Some(b'\\') => ('\\', 2),
                        Some(b'\'') => ('\'', 2),
                        Some(b'"') => ('"', 2),
                        Some(b'n') => ('\n', 2),
                        Some(b'r') => ('\r', 2),
                        Some(b't') => ('\t', 2),
                        Some(b'u') if self.s.get(self.i + 2) == Some(&b'{') => {
                            let rest = &self.s[self.i + 3..];
                            let n = rest.iter().position(|&x| x == b'}').unwrap_or(usize::MAX);
                            let digits = rest.get(..n).and_then(|d| core::str::from_utf8(d).ok());
                            let v = digits
                                .filter(|d| (1..=6).contains(&d.len()))
                                .and_then(|d| u32::from_str_radix(d, 16).ok())
                                .and_then(char::from_u32);
                            match v {
                                Some(ch) => (ch, n + 4),
                                None => return self.fail("a bad \\u{…} escape (E003)"),
                            }
                        }
                        _ => return self.fail("an escape LQ does not have (E003)"),
                    };
                    val.push(ch);
                    self.i += n;
                }
                _ => {
                    let rest = core::str::from_utf8(&self.s[self.i..]).map_err(|_| "not UTF-8")?;
                    let ch = rest.chars().next().expect("non-empty");
                    if ch != '\t' && (ch.is_control() || ch == '\u{7f}') {
                        return self.fail("a control character inside a string (E003)");
                    }
                    val.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
        if q == b'`' && val.is_empty() {
            self.i = start;
            return self.fail("an empty back-quoted name (E001)");
        }
        let src = String::from_utf8(self.s[start..self.i].to_vec()).map_err(|_| "not UTF-8")?;
        Ok((src, val))
    }

    /// The next token, or `None` at the end.
    fn next(&mut self) -> Result<Option<Tok>, String> {
        self.skip()?;
        let Some(&c) = self.s.get(self.i) else {
            return Ok(None);
        };
        let start = self.i;
        let word_end = |s: &[u8], mut j: usize| {
            while j < s.len() && is_word(s[j]) {
                j += 1;
            }
            j
        };
        let tok = if is_word_start(c) {
            self.i = word_end(self.s, self.i);
            Tok::Word(String::from_utf8(self.s[start..self.i].to_vec()).expect("ASCII"))
        } else if c == b'`' {
            Tok::Quoted(self.delimited(b'`')?.1)
        } else if c == b'\'' || c == b'"' {
            Tok::Lit(self.delimited(c)?.0)
        } else if c == b'$' {
            if !self.s.get(self.i + 1).copied().is_some_and(is_word_start) {
                return self.fail("`$` not followed by a word (E001)");
            }
            self.i = word_end(self.s, self.i + 1);
            Tok::Param(String::from_utf8(self.s[start + 1..self.i].to_vec()).expect("ASCII"))
        } else if c.is_ascii_digit() {
            self.number()?;
            Tok::Lit(String::from_utf8(self.s[start..self.i].to_vec()).expect("ASCII"))
        } else if c == b'#' {
            self.node_literal()?;
            Tok::Lit(String::from_utf8(self.s[start..self.i].to_vec()).expect("ASCII"))
        } else {
            let rest = &self.s[self.i..];
            let Some(p) = PUNCT.iter().find(|p| {
                rest.starts_with(p.as_bytes())
                    && (**p != "<-" || matches!(rest.get(2), Some(b'[' | b'-')))
            }) else {
                return self.fail("a character outside the token set (E001)");
            };
            self.i += p.len();
            Tok::Punct(p)
        };
        Ok(Some(tok))
    }

    /// [LQ/lexical §5.6] at a digit.
    fn number(&mut self) -> Result<(), String> {
        let s = self.s;
        let digits = |mut j: usize| {
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            j
        };
        let start = self.i;
        self.i = digits(self.i);
        let is_digit_at = |j: usize| s.get(j).is_some_and(u8::is_ascii_digit);
        let mut float = false;
        if s.get(self.i) == Some(&b'.') && is_digit_at(self.i + 1) {
            self.i = digits(self.i + 1);
            float = true;
        }
        if matches!(s.get(self.i), Some(b'e' | b'E')) {
            let j = self.i + 1;
            let k = if matches!(s.get(j), Some(b'+' | b'-')) {
                j + 1
            } else {
                j
            };
            if is_digit_at(k) {
                self.i = digits(k);
                float = true;
            }
        }
        if !float
            && matches!(s.get(self.i), Some(b's' | b'm' | b'h' | b'd' | b'w'))
            && !s.get(self.i + 1).copied().is_some_and(is_word)
        {
            self.i += 1;
        }
        if s.get(self.i).copied().is_some_and(is_word) {
            self.i = start;
            return self.fail("a letter, digit or `_` directly after a number (E003)");
        }
        Ok(())
    }

    /// [LQ/lexical §5.8] at `#`.
    fn node_literal(&mut self) -> Result<(), String> {
        let s = self.s;
        let start = self.i;
        let j = self.i + 1;
        if s.get(j..j + 2) == Some(b"u:") {
            let hex = s.get(j + 2..j + 34).unwrap_or(&[]);
            if hex.len() == 32
                && hex
                    .iter()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            {
                self.i = j + 34;
            } else {
                return self.fail("`#u:` not followed by 32 lower-case hex digits (E003)");
            }
        } else if s.get(j).is_some_and(u8::is_ascii_digit) {
            let mut k = j;
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            let v: u64 = core::str::from_utf8(&s[j..k])
                .ok()
                .and_then(|d| d.parse().ok())
                .unwrap_or(u64::MAX);
            if v == 0 || v > u64::from(u32::MAX) {
                return self.fail("a node literal outside 1-4294967295 (E003)");
            }
            self.i = k;
        } else {
            return self.fail("`#` not followed by digits or `u:` (E003)");
        }
        if s.get(self.i).copied().is_some_and(is_word) {
            self.i = start;
            return self.fail("a letter, digit or `_` directly after a node or uid literal (E003)");
        }
        Ok(())
    }
}

/// A `qname` segment in the canonical spelling ([F14 §7.2.1], [LQ/lexical §5.4]).
fn spell(seg: &str) -> String {
    let b = seg.as_bytes();
    if !b.is_empty() && is_word_start(b[0]) && b.iter().all(|&c| is_word(c)) {
        seg.to_owned()
    } else {
        format!("`{}`", seg.replace('`', "``"))
    }
}

fn is_kw(t: &Option<Tok>, kw: &str) -> bool {
    matches!(t, Some(Tok::Word(w)) if w.eq_ignore_ascii_case(kw))
}

/// Parses a stored query text through the head of its `define_stmt` and checks that the body after `AS {` closes with
/// the text's last token.
pub fn define_head(text: &str) -> Result<DefineHead, String> {
    let mut lx = Lexer::new(text);
    let mut t = lx.next()?;
    if !is_kw(&t, "DEFINE") {
        return Err("the text does not begin with DEFINE [LQ/grammar-v1.ebnf define_stmt]".into());
    }
    t = lx.next()?;
    if !is_kw(&t, "QUERY") {
        return Err("DEFINE is not followed by QUERY [LQ/grammar-v1.ebnf define_stmt]".into());
    }
    let ident = |t: Option<Tok>| match t {
        Some(Tok::Word(w) | Tok::Quoted(w)) => Ok(w),
        other => Err(format!(
            "{other:?} where an ident is expected [LQ/grammar-v1.ebnf]"
        )),
    };
    let mut segs = vec![ident(lx.next()?)?];
    t = lx.next()?;
    while t == Some(Tok::Punct(".")) {
        segs.push(ident(lx.next()?)?);
        t = lx.next()?;
    }
    let name = segs.iter().map(|s| spell(s)).collect::<Vec<_>>().join(".");
    if t != Some(Tok::Punct("(")) {
        return Err("the qname is not followed by ( [LQ/grammar-v1.ebnf define_stmt]".into());
    }
    let mut decls = Vec::new();
    t = lx.next()?;
    if t != Some(Tok::Punct(")")) {
        loop {
            let Some(Tok::Param(p)) = t else {
                return Err("a param_decl does not begin with $name [LQ/grammar-v1.ebnf]".into());
            };
            if lx.next()? != Some(Tok::Punct(":")) {
                return Err("a param_decl lacks `:` [LQ/grammar-v1.ebnf]".into());
            }
            let mut ty = ident(lx.next()?)?;
            t = lx.next()?;
            if t == Some(Tok::Punct("<")) {
                let inner = ident(lx.next()?)?;
                if lx.next()? != Some(Tok::Punct(">")) {
                    return Err(
                        "a param type's < is not closed by > [LQ/grammar-v1.ebnf type]".into(),
                    );
                }
                ty = format!("{ty}<{inner}>");
                t = lx.next()?;
            }
            let mut decl = format!("${p}: {ty}");
            if t == Some(Tok::Punct("?")) {
                decl.push('?');
                t = lx.next()?;
            }
            if t == Some(Tok::Punct("=")) {
                let d = match lx.next()? {
                    Some(Tok::Lit(l)) => l,
                    Some(Tok::Word(w))
                        if ["TRUE", "FALSE", "NULL"]
                            .iter()
                            .any(|k| w.eq_ignore_ascii_case(k)) =>
                    {
                        w
                    }
                    other => {
                        return Err(format!(
                            "{other:?} is not a literal or node_lit default [LQ/grammar-v1.ebnf param_decl]"
                        ));
                    }
                };
                decl.push_str(" = ");
                decl.push_str(&d);
                t = lx.next()?;
            }
            decls.push(decl);
            match t {
                Some(Tok::Punct(",")) => t = lx.next()?,
                Some(Tok::Punct(")")) => break,
                _ => {
                    return Err(
                        "a param_decl is not followed by `,` or `)` [LQ/grammar-v1.ebnf]".into(),
                    );
                }
            }
        }
    }
    t = lx.next()?;
    let mut shape = None;
    let mut budget = None;
    if is_kw(&t, "SHAPE") {
        shape = Some(ident(lx.next()?)?);
        t = lx.next()?;
    }
    if is_kw(&t, "BUDGET") {
        budget = Some(ident(lx.next()?)?);
        t = lx.next()?;
    }
    if !is_kw(&t, "AS") || lx.next()? != Some(Tok::Punct("{")) {
        return Err("the head is not followed by AS { [LQ/grammar-v1.ebnf define_stmt]".into());
    }
    let mut depth = 1usize;
    while depth > 0 {
        match lx.next()? {
            Some(Tok::Punct("{")) => depth += 1,
            Some(Tok::Punct("}")) => depth -= 1,
            Some(_) => {}
            None => {
                return Err("the query body after AS { is not closed [LQ/grammar-v1.ebnf]".into());
            }
        }
    }
    if lx.next()?.is_some() {
        return Err("tokens after the define_stmt's closing } [LQ/grammar-v1.ebnf]".into());
    }
    Ok(DefineHead {
        name,
        params: decls.join(", "),
        shape,
        budget,
    })
}

/// [LQ/lexical §10.2]'s pre-check: a node literal token (`#` and digits) outside strings, back-quoted names and comments
/// is a node-typed constant other than a `#u:` literal, so the text is not portable.
pub fn has_node_literal(text: &str) -> Result<bool, String> {
    let mut lx = Lexer::new(text);
    while let Some(t) = lx.next()? {
        if let Tok::Lit(l) = t
            && l.starts_with('#')
            && !l.starts_with("#u:")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: &str = "DEFINE QUERY lane_ready($scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20, $ids: list<node>? = NULL) SHAPE node BUDGET light AS {\n  MATCH (t:task) // a comment with #12\n  WHERE t.title <> 'x#3' AND t IN subtree($scope)\n  RETURN t LIMIT $limit\n}";

    /// [F14 §7.2.2], §7.2.3: the head, the rendered signature, SHAPE and BUDGET; comments and strings hide `#N`.
    #[test]
    fn head_and_signature() {
        let h = define_head(Q).unwrap();
        assert_eq!(h.name, "lane_ready");
        assert_eq!(
            h.params,
            "$scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20, $ids: list<node>? = NULL"
        );
        assert_eq!(h.shape.as_deref(), Some("node"));
        assert_eq!(h.budget.as_deref(), Some("light"));
        assert!(!has_node_literal(Q).unwrap());
        assert!(has_node_literal(&Q.replace("$scope)", "#40)")).unwrap());
        let quoted = define_head("define query std.`my q`() as { RETURN 1 }").unwrap();
        assert_eq!(quoted.name, "std.`my q`");
        assert_eq!(quoted.params, "");
        assert_eq!(quoted.budget, None);
    }

    /// Texts that are not a `define_stmt` head, or whose body does not close, are refused.
    #[test]
    fn refusals() {
        assert!(define_head("DEFINE open() AS { RETURN 1 }").is_err());
        assert!(define_head("DEFINE QUERY q() AS { RETURN 1 ").is_err());
        assert!(define_head("DEFINE QUERY q() AS { RETURN 1 } x").is_err());
        assert!(define_head("DEFINE QUERY q($a int) AS { }").is_err());
        assert!(define_head("DEFINE QUERY q() AS { RETURN 'x }").is_err());
        assert!(define_head("DEFINE QUERY q() AS { RETURN #0 }").is_err());
    }
}
