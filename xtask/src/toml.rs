//! A TOML reader for the repository's own configuration files and for `Cargo.lock` and `Cargo.toml`.
//!
//! The tool crate may depend only on `serde_json`, `blake3`, `xxhash-rust` and `sha2` (docs/m0/PLAN.md §2.2), so the
//! TOML it reads is parsed here. It covers TOML 1.0 except date-times, which none of those files use: comments,
//! bare, quoted and dotted keys, `[table]` and `[[array.of.tables]]` headers, basic and literal strings (single and
//! multi-line, with every escape), integers (decimal, hex, octal, binary, `_` separators), floats, booleans, arrays and
//! inline tables. A document that uses anything else, or defines a key twice, is refused with its line number.

use std::collections::BTreeMap;
use std::fmt;

/// A parsed TOML value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<Value>),
    Table(Table),
}

/// A TOML table; keys are kept in sorted order.
pub type Table = BTreeMap<String, Value>;

/// A parse error with its 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
    /// Follows a dotted path of table keys.
    pub fn get_path(&self, path: &[&str]) -> Option<&Value> {
        let mut v = self;
        for k in path {
            v = v.as_table()?.get(*k)?;
        }
        Some(v)
    }
}

/// Parses a whole TOML document into its root table.
pub fn parse(text: &str) -> Result<Table, Error> {
    let mut p = Parser {
        s: text.as_bytes(),
        text,
        pos: 0,
        line: 1,
    };
    p.document()
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
    line: usize,
}

/// Where a header or dotted key was defined, so a second definition is refused.
#[derive(Default)]
struct Defined {
    /// Tables created by a `[header]` (as dotted paths, with array indices).
    headers: std::collections::BTreeSet<String>,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        Err(Error {
            line: self.line,
            message: message.into(),
        })
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.s.get(self.pos + off).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn starts_with(&self, lit: &str) -> bool {
        self.s[self.pos..].starts_with(lit.as_bytes())
    }

    /// Spaces and tabs only.
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn comment(&mut self) {
        if self.peek() == Some(b'#') {
            while let Some(c) = self.peek() {
                if c == b'\n' {
                    break;
                }
                self.pos += 1;
            }
        }
    }

    /// Whitespace, newlines and comments (inside arrays).
    fn ws_nl_comments(&mut self) {
        loop {
            self.ws();
            self.comment();
            match self.peek() {
                Some(b'\n') => {
                    self.bump();
                }
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
    }

    /// The end of a key/value line or header: optional comment, then a newline or the end of input.
    fn line_end(&mut self) -> Result<(), Error> {
        self.ws();
        self.comment();
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.bump();
                Ok(())
            }
            Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                self.pos += 1;
                self.bump();
                Ok(())
            }
            Some(c) => self.err(format!("unexpected '{}' after a value", c as char)),
        }
    }

    fn document(&mut self) -> Result<Table, Error> {
        let mut root = Table::new();
        let mut defined = Defined::default();
        // The current table: a path of keys; an array of tables in the path means its last element.
        let mut current: Vec<String> = Vec::new();
        loop {
            self.ws_nl_comments();
            let Some(c) = self.peek() else { break };
            if c == b'[' {
                let array = self.peek_at(1) == Some(b'[');
                self.pos += if array { 2 } else { 1 };
                self.ws();
                let key = self.key()?;
                self.ws();
                if array {
                    if !self.starts_with("]]") {
                        return self.err("expected ']]' to close an array-of-tables header");
                    }
                    self.pos += 2;
                } else {
                    if self.peek() != Some(b']') {
                        return self.err("expected ']' to close a table header");
                    }
                    self.pos += 1;
                }
                self.line_end()?;
                if array {
                    let (parent, last) = key.split_at(key.len() - 1);
                    let table = descend(&mut root, parent, self.line)?;
                    let entry = table
                        .entry(last[0].clone())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    match entry {
                        Value::Array(items) => items.push(Value::Table(Table::new())),
                        _ => {
                            return self
                                .err(format!("'{}' is not an array of tables", key.join(".")));
                        }
                    }
                } else {
                    let path = header_id(&root, &key);
                    if !defined.headers.insert(path) {
                        return self.err(format!("table [{}] is defined twice", key.join(".")));
                    }
                    let t = descend(&mut root, &key, self.line)?;
                    let _ = t;
                }
                current = key;
            } else {
                let key = self.key()?;
                self.ws();
                if self.peek() != Some(b'=') {
                    return self.err("expected '=' after a key");
                }
                self.pos += 1;
                self.ws();
                let value = self.value()?;
                self.line_end()?;
                let line = self.line;
                let table = descend(&mut root, &current, line)?;
                insert_dotted(table, &key, value, line)?;
            }
        }
        Ok(root)
    }

    /// A dotted key.
    fn key(&mut self) -> Result<Vec<String>, Error> {
        let mut parts = Vec::new();
        loop {
            self.ws();
            let part = match self.peek() {
                Some(b'"') => self.basic_string()?,
                Some(b'\'') => self.literal_string()?,
                Some(c) if is_bare(c) => {
                    let start = self.pos;
                    while matches!(self.peek(), Some(c) if is_bare(c)) {
                        self.pos += 1;
                    }
                    self.text[start..self.pos].to_string()
                }
                _ => return self.err("expected a key"),
            };
            parts.push(part);
            self.ws();
            if self.peek() == Some(b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(parts)
    }

    fn value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some(b'"') => {
                if self.starts_with("\"\"\"") {
                    self.ml_basic_string().map(Value::String)
                } else {
                    self.basic_string().map(Value::String)
                }
            }
            Some(b'\'') => {
                if self.starts_with("'''") {
                    self.ml_literal_string().map(Value::String)
                } else {
                    self.literal_string().map(Value::String)
                }
            }
            Some(b'[') => self.array(),
            Some(b'{') => self.inline_table(),
            Some(b't') if self.starts_with("true") => {
                self.pos += 4;
                Ok(Value::Boolean(true))
            }
            Some(b'f') if self.starts_with("false") => {
                self.pos += 5;
                Ok(Value::Boolean(false))
            }
            Some(c) if c == b'+' || c == b'-' || c.is_ascii_digit() || c == b'i' || c == b'n' => {
                self.number()
            }
            _ => self.err("expected a value"),
        }
    }

    fn array(&mut self) -> Result<Value, Error> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        loop {
            self.ws_nl_comments();
            if self.peek() == Some(b']') {
                self.pos += 1;
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
            self.ws_nl_comments();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return self.err("expected ',' or ']' in an array"),
            }
        }
    }

    fn inline_table(&mut self) -> Result<Value, Error> {
        self.pos += 1; // '{'
        let mut table = Table::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Table(table));
        }
        loop {
            let key = self.key()?;
            self.ws();
            if self.peek() != Some(b'=') {
                return self.err("expected '=' in an inline table");
            }
            self.pos += 1;
            self.ws();
            let v = self.value()?;
            insert_dotted(&mut table, &key, v, self.line)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.ws();
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Table(table));
                }
                _ => return self.err("expected ',' or '}' in an inline table"),
            }
        }
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'_' | b'.'))
        {
            self.pos += 1;
        }
        let raw = &self.text[start..self.pos];
        let clean: String = raw.chars().filter(|&c| c != '_').collect();
        let (sign, body) = match clean.as_bytes().first() {
            Some(b'+') => (1i64, &clean[1..]),
            Some(b'-') => (-1i64, &clean[1..]),
            _ => (1i64, &clean[..]),
        };
        let radix = |p: &str, r: u32| i64::from_str_radix(p, r).ok();
        let int = if let Some(h) = body.strip_prefix("0x") {
            radix(h, 16)
        } else if let Some(o) = body.strip_prefix("0o") {
            radix(o, 8)
        } else if let Some(b) = body.strip_prefix("0b") {
            radix(b, 2)
        } else if !body.is_empty() && body.bytes().all(|c| c.is_ascii_digit()) {
            body.parse::<i64>().ok()
        } else {
            None
        };
        if let Some(i) = int {
            return Ok(Value::Integer(sign * i));
        }
        match body {
            "inf" => return Ok(Value::Float(sign as f64 * f64::INFINITY)),
            "nan" => return Ok(Value::Float(f64::NAN)),
            _ => {}
        }
        if body.contains(':') || (body.len() >= 10 && body.as_bytes().get(4) == Some(&b'-')) {
            return self.err(format!("date-times are not supported: '{raw}'"));
        }
        match clean.parse::<f64>() {
            Ok(f) => Ok(Value::Float(f)),
            Err(_) => self.err(format!("invalid number '{raw}'")),
        }
    }

    fn basic_string(&mut self) -> Result<String, Error> {
        self.pos += 1; // '"'
        let mut out = String::new();
        loop {
            let start = self.pos;
            while matches!(self.peek(), Some(c) if c != b'"' && c != b'\\' && c != b'\n') {
                self.pos += 1;
            }
            out.push_str(&self.text[start..self.pos]);
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => self.escape(&mut out)?,
                _ => return self.err("unterminated string"),
            }
        }
    }

    fn ml_basic_string(&mut self) -> Result<String, Error> {
        self.pos += 3;
        self.skip_first_newline();
        let mut out = String::new();
        loop {
            if self.starts_with("\"\"\"") {
                // Up to two quotes may directly precede the closing delimiter.
                let mut n = 3;
                while self.peek_at(n) == Some(b'"') && n < 5 {
                    n += 1;
                }
                for _ in 3..n {
                    out.push('"');
                }
                self.pos += n;
                return Ok(out);
            }
            match self.peek() {
                None => return self.err("unterminated multi-line string"),
                Some(b'\\') => {
                    // A line-ending backslash trims the newline and following whitespace.
                    let mut k = 1;
                    while matches!(self.peek_at(k), Some(b' ' | b'\t')) {
                        k += 1;
                    }
                    if matches!(self.peek_at(k), Some(b'\n' | b'\r')) {
                        self.pos += k;
                        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                            self.bump();
                        }
                    } else {
                        self.escape(&mut out)?;
                    }
                }
                Some(_) => {
                    let ch = self.next_char();
                    out.push(ch);
                }
            }
        }
    }

    fn literal_string(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c != b'\'' && c != b'\n') {
            self.pos += 1;
        }
        if self.peek() != Some(b'\'') {
            return self.err("unterminated literal string");
        }
        let s = self.text[start..self.pos].to_string();
        self.pos += 1;
        Ok(s)
    }

    fn ml_literal_string(&mut self) -> Result<String, Error> {
        self.pos += 3;
        self.skip_first_newline();
        let start = self.pos;
        loop {
            if self.starts_with("'''") {
                let mut n = 3;
                while self.peek_at(n) == Some(b'\'') && n < 5 {
                    n += 1;
                }
                let s = self.text[start..self.pos + n - 3].to_string();
                self.pos += n;
                return Ok(s);
            }
            if self.bump().is_none() {
                return self.err("unterminated multi-line literal string");
            }
        }
    }

    fn skip_first_newline(&mut self) {
        if self.peek() == Some(b'\n') {
            self.bump();
        } else if self.starts_with("\r\n") {
            self.pos += 1;
            self.bump();
        }
    }

    fn next_char(&mut self) -> char {
        let ch = self.text[self.pos..].chars().next().unwrap_or('\u{fffd}');
        for _ in 0..ch.len_utf8() {
            self.bump();
        }
        ch
    }

    fn escape(&mut self, out: &mut String) -> Result<(), Error> {
        self.pos += 1; // '\\'
        let c = match self.bump() {
            Some(c) => c,
            None => return self.err("unterminated escape"),
        };
        match c {
            b'b' => out.push('\u{8}'),
            b't' => out.push('\t'),
            b'n' => out.push('\n'),
            b'f' => out.push('\u{c}'),
            b'r' => out.push('\r'),
            b'e' => out.push('\u{1b}'),
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'u' | b'U' => {
                let n = if c == b'u' { 4 } else { 8 };
                if self.pos + n > self.s.len() {
                    return self.err("short unicode escape");
                }
                let hex = &self.text[self.pos..self.pos + n];
                self.pos += n;
                let code = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                match code {
                    Some(ch) => out.push(ch),
                    None => {
                        return self.err(format!("invalid unicode escape '\\{}{hex}'", c as char));
                    }
                }
            }
            other => return self.err(format!("invalid escape '\\{}'", other as char)),
        }
        Ok(())
    }
}

fn is_bare(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// A stable identity for a `[header]` path: array-of-tables segments carry the index of their last element.
fn header_id(root: &Table, key: &[String]) -> String {
    let mut id = String::new();
    let mut t = Some(root);
    for k in key {
        if !id.is_empty() {
            id.push('.');
        }
        id.push_str(k);
        match t.and_then(|t| t.get(k)) {
            Some(Value::Array(items)) => {
                id.push_str(&format!("[{}]", items.len().saturating_sub(1)));
                t = items.last().and_then(Value::as_table);
            }
            Some(Value::Table(inner)) => t = Some(inner),
            _ => t = None,
        }
    }
    id
}

/// Walks (and creates) the tables of `path`; an array of tables stands for its last element.
fn descend<'t>(root: &'t mut Table, path: &[String], line: usize) -> Result<&'t mut Table, Error> {
    let mut t = root;
    for k in path {
        let entry = t
            .entry(k.clone())
            .or_insert_with(|| Value::Table(Table::new()));
        t = match entry {
            Value::Table(inner) => inner,
            Value::Array(items) => match items.last_mut() {
                Some(Value::Table(inner)) => inner,
                _ => {
                    return Err(Error {
                        line,
                        message: format!("'{k}' is an array, not a table"),
                    });
                }
            },
            _ => {
                return Err(Error {
                    line,
                    message: format!("'{k}' is a value, not a table"),
                });
            }
        };
    }
    Ok(t)
}

fn insert_dotted(
    table: &mut Table,
    key: &[String],
    value: Value,
    line: usize,
) -> Result<(), Error> {
    let (parents, last) = key.split_at(key.len() - 1);
    let t = descend(table, parents, line)?;
    if t.contains_key(&last[0]) {
        return Err(Error {
            line,
            message: format!("key '{}' is defined twice", key.join(".")),
        });
    }
    t.insert(last[0].clone(), value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_and_strings() {
        let t = parse(
            "a = 1\nb = -0x1F\nc = 1_000\nd = true\ne = \"x\\ty\\u00e9\"\nf = 'C:\\raw'\ng = 1.5\n# c\nh = \"\"\"\nline1\\\n   line2\"\"\"\ni = '''\nq'x'''\n",
        )
        .unwrap();
        assert_eq!(t["a"], Value::Integer(1));
        assert_eq!(t["b"], Value::Integer(-31));
        assert_eq!(t["c"], Value::Integer(1000));
        assert_eq!(t["d"], Value::Boolean(true));
        assert_eq!(t["e"], Value::String("x\ty\u{e9}".into()));
        assert_eq!(t["f"], Value::String("C:\\raw".into()));
        assert_eq!(t["g"], Value::Float(1.5));
        assert_eq!(t["h"], Value::String("line1line2".into()));
        assert_eq!(t["i"], Value::String("q'x".into()));
    }

    #[test]
    fn tables_arrays_and_inline() {
        let t = parse(
            "version = 4\n\n[[package]]\nname = \"a\"\ndependencies = [\n \"b\", # note\n \"c 1.0\",\n]\n\n[[package]]\nname = \"b\"\n\n[x.y]\nz = { p = 1, q.r = \"s\" }\n[x]\nw = [[1, 2], []]\n",
        )
        .unwrap();
        let pk = t["package"].as_array().unwrap();
        assert_eq!(pk.len(), 2);
        assert_eq!(pk[0].get_path(&["name"]).unwrap().as_str(), Some("a"));
        assert_eq!(
            pk[0]
                .get_path(&["dependencies"])
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let root = Value::Table(t.clone());
        assert_eq!(
            root.get_path(&["x", "y", "z", "q", "r"]).unwrap().as_str(),
            Some("s")
        );
        assert_eq!(
            root.get_path(&["x", "w"])
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn nested_array_of_tables_and_quoted_keys() {
        let t = parse("[[a]]\n[a.b]\nc = 1\n[[a]]\n[a.b]\nc = 2\n[\"q.k\"]\n'l k' = 3\n[target.'cfg(windows)'.dependencies]\nw = \"1\"\n")
            .unwrap();
        let a = t["a"].as_array().unwrap();
        assert_eq!(a[1].get_path(&["b", "c"]), Some(&Value::Integer(2)));
        assert_eq!(t["q.k"].as_table().unwrap()["l k"], Value::Integer(3));
        let root = Value::Table(t);
        assert!(
            root.get_path(&["target", "cfg(windows)", "dependencies", "w"])
                .is_some()
        );
    }

    #[test]
    fn refusals() {
        assert!(parse("a = 1\na = 2\n").is_err());
        assert!(parse("[t]\n[t]\n").is_err());
        assert!(parse("a = 1979-05-27T07:32:00Z\n").is_err());
        assert!(parse("a = \"open\n").is_err());
        assert!(parse("a = 1 b = 2\n").is_err());
        let e = parse("\n\nx = \n").unwrap_err();
        assert_eq!(e.line, 3);
    }

    #[test]
    fn crlf_documents() {
        let t = parse("a = 1\r\n[b]\r\nc = [\r\n  1,\r\n]\r\n").unwrap();
        assert_eq!(t["a"], Value::Integer(1));
    }
}
