//! SPDX licence expressions for the licence lint (docs/m0/PLAN.md §2.4; `xtask/licence-allow.toml`).
//!
//! Grammar (SPDX 2.3 annex D, with the legacy `/` that crates.io still carries read as `OR`):
//! `or := and ((OR | "/") and)*`, `and := term (AND term)*`, `term := "(" or ")" | id ["+"] [WITH id]`. Operators are
//! matched case-insensitively. A `WITH` term is one licence and must be allowed whole.
//!
//! Evaluation: every `AND` term must be allowed, and at least one alternative of each `OR`.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A licence identifier, possibly `X WITH Y` or `X+`.
    Term(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Term(t) => f.write_str(t),
            Expr::And(v) | Expr::Or(v) => {
                let op = if matches!(self, Expr::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                f.write_str("(")?;
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        f.write_str(op)?;
                    }
                    write!(f, "{e}")?;
                }
                f.write_str(")")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Open,
    Close,
    And,
    Or,
    With,
    Id(String),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' => {
                out.push(Tok::Open);
                i += 1;
            }
            b')' => {
                out.push(Tok::Close);
                i += 1;
            }
            b'/' => {
                out.push(Tok::Or);
                i += 1;
            }
            _ if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+' | b':') => {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'.' | b'-' | b'+' | b':'))
                {
                    i += 1;
                }
                let w = &s[start..i];
                out.push(match w.to_ascii_uppercase().as_str() {
                    "AND" => Tok::And,
                    "OR" => Tok::Or,
                    "WITH" => Tok::With,
                    _ => Tok::Id(w.to_string()),
                });
            }
            _ => {
                return Err(format!(
                    "unexpected character '{}' in licence expression '{s}'",
                    c as char
                ));
            }
        }
    }
    Ok(out)
}

/// Parses an SPDX expression.
pub fn parse(s: &str) -> Result<Expr, String> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err("empty licence expression".into());
    }
    let mut p = P {
        t: &toks,
        i: 0,
        src: s,
    };
    let e = p.or()?;
    if p.i != toks.len() {
        return Err(format!("trailing tokens in licence expression '{s}'"));
    }
    Ok(e)
}

struct P<'a> {
    t: &'a [Tok],
    i: usize,
    src: &'a str,
}

impl P<'_> {
    fn or(&mut self) -> Result<Expr, String> {
        let mut v = vec![self.and()?];
        while self.t.get(self.i) == Some(&Tok::Or) {
            self.i += 1;
            v.push(self.and()?);
        }
        Ok(if v.len() == 1 {
            v.pop().unwrap_or(Expr::Or(Vec::new()))
        } else {
            Expr::Or(v)
        })
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut v = vec![self.term()?];
        while self.t.get(self.i) == Some(&Tok::And) {
            self.i += 1;
            v.push(self.term()?);
        }
        Ok(if v.len() == 1 {
            v.pop().unwrap_or(Expr::And(Vec::new()))
        } else {
            Expr::And(v)
        })
    }

    fn term(&mut self) -> Result<Expr, String> {
        match self.t.get(self.i) {
            Some(Tok::Open) => {
                self.i += 1;
                let e = self.or()?;
                if self.t.get(self.i) != Some(&Tok::Close) {
                    return Err(format!(
                        "unbalanced parenthesis in licence expression '{}'",
                        self.src
                    ));
                }
                self.i += 1;
                Ok(e)
            }
            Some(Tok::Id(id)) => {
                self.i += 1;
                let mut term = id.clone();
                if self.t.get(self.i) == Some(&Tok::With) {
                    self.i += 1;
                    match self.t.get(self.i) {
                        Some(Tok::Id(ex)) => {
                            self.i += 1;
                            term = format!("{term} WITH {ex}");
                        }
                        _ => return Err(format!("WITH without an exception in '{}'", self.src)),
                    }
                }
                Ok(Expr::Term(term))
            }
            _ => Err(format!("expected a licence identifier in '{}'", self.src)),
        }
    }
}

/// Evaluates `e` against `allowed` (compared ASCII-case-insensitively, with `WITH` spacing normalised). Returns the
/// terms that made the expression fail, or `Ok` when it is satisfied.
pub fn check(e: &Expr, allowed: &[&str]) -> Result<(), Vec<String>> {
    match e {
        Expr::Term(t) => {
            let norm = normalise(t);
            if allowed.iter().any(|a| normalise(a) == norm) {
                Ok(())
            } else {
                Err(vec![t.clone()])
            }
        }
        Expr::And(v) => {
            let mut bad = Vec::new();
            for x in v {
                if let Err(b) = check(x, allowed) {
                    bad.extend(b);
                }
            }
            if bad.is_empty() { Ok(()) } else { Err(bad) }
        }
        Expr::Or(v) => {
            let mut bad = Vec::new();
            for x in v {
                match check(x, allowed) {
                    Ok(()) => return Ok(()),
                    Err(b) => bad.extend(b),
                }
            }
            Err(bad)
        }
    }
}

fn normalise(s: &str) -> String {
    s.split_whitespace()
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWED: &[&str] = &[
        "MIT",
        "Apache-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "Unicode-3.0",
    ];

    fn ok(s: &str) -> bool {
        check(&parse(s).unwrap(), ALLOWED).is_ok()
    }

    #[test]
    fn expressions() {
        assert!(ok("MIT"));
        assert!(ok("MIT OR Apache-2.0"));
        assert!(ok("MIT/Apache-2.0"));
        assert!(ok("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert!(ok("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"));
        assert!(ok("mit or apache-2.0"));
        assert!(!ok("GPL-3.0"));
        assert!(!ok("MIT AND GPL-3.0"));
        assert!(ok("GPL-3.0 OR MIT"));
        assert!(!ok("MIT WITH Foo-exception"));
        assert!(!ok("MIT+"));
        let e = check(&parse("MIT AND (GPL-2.0 OR LGPL-2.1)").unwrap(), ALLOWED).unwrap_err();
        assert_eq!(e, vec!["GPL-2.0".to_string(), "LGPL-2.1".to_string()]);
    }

    #[test]
    fn malformed() {
        assert!(parse("").is_err());
        assert!(parse("(MIT").is_err());
        assert!(parse("MIT AND").is_err());
        assert!(parse("MIT WITH").is_err());
        assert!(parse("MIT, Apache-2.0").is_err());
    }
}
