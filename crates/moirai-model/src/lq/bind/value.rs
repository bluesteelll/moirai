//! Constants ([LQ/canonical-ast §5.5]–§5.6): node literals and ids (E111), type-directed coercion of literals and bare
//! words (E102, E108), parameter values by their use site (E110, [LQ/std §2.2]), revisions resolved to full commit ids
//! (E301) and the store-local constants a definition may not hold (E117, the portable rewrite of §8.1).

use super::Binder;
use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::{EnumTy, Ty};
use crate::lq::ctx::Value;
use crate::lq::diag::{Code, Diag, Span, list, near, q, value as cap};
use crate::lq::lexer::{Lexer, RevRead, datetime_ms, duration_ms, iso_datetime, parse_decimal};
use crate::lq::printer;
use crate::lq::schema::KindSet;

/// Lower-case hex of bytes.
pub(super) fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn unhex<const N: usize>(h: &str) -> Option<[u8; N]> {
    if h.len() != 2 * N || !h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// A uid from its 32 lower-case hex digits.
pub(super) fn parse_uid(h: &str) -> Option<Uid> {
    unhex::<16>(h)
}

/// A node named in text ([LQ/std §2.2]: `88`, `#88`, `#u:<32 hex>`).
pub(super) enum NodeRef {
    /// `#N`.
    Num(u32),
    /// A uid.
    Uid(Uid),
}

/// Reads a node value from `k=v` text.
pub(super) fn node_text(t: &str) -> Option<NodeRef> {
    if let Some(h) = t.strip_prefix("#u:") {
        return parse_uid(h).map(NodeRef::Uid);
    }
    let d = t.strip_prefix('#').unwrap_or(t);
    parse_decimal(d)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n >= 1)
        .map(NodeRef::Num)
}

/// A value as texts print it.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Text(t) => t.clone(),
        Value::List(v) => format!(
            "[{}]",
            v.iter().map(value_text).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// A `range<int>` in `k=v` text: `a..b`, `..b`, `a..`, or `a` meaning `a..a` ([LQ/std §2.2]).
fn range_text(t: &str) -> Option<(Option<i64>, Option<i64>)> {
    let int = |s: &str| -> Option<Option<i64>> {
        if s.is_empty() {
            Some(None)
        } else {
            let (neg, d) = s.strip_prefix('-').map_or((false, s), |d| (true, d));
            let n = i64::try_from(parse_decimal(d)?).ok()?;
            Some(Some(if neg { -n } else { n }))
        }
    };
    match t.split_once("..") {
        Some((a, b)) => {
            let (a, b) = (int(a)?, int(b)?);
            (a.is_some() || b.is_some()).then_some((a, b))
        }
        None => int(t)?.map(|a| (Some(a), Some(a))),
    }
}

/// The innermost base of a revision (suffix chains are walked in a loop, [LQ/grammar-v1.ebnf §P.13]).
fn base_of(r: &Rev) -> &Rev {
    let mut r = r;
    while let RevKind::Suf(b, _) = &r.kind {
        r = b;
    }
    r
}

/// Whether a revision's base is store-local: a sequence number or a commit prefix ([LQ/canonical-ast §8.1] step 4).
fn short_base(r: &Rev) -> bool {
    match &base_of(r).kind {
        RevKind::Seq(_) => true,
        RevKind::Commit(h) => h.len() < 64,
        _ => false,
    }
}

impl Binder<'_> {
    /// Records a portable rewrite of a definition's text ([LQ/canonical-ast §8.1] step 4).
    fn rewrite(&mut self, span: Span, text: String) {
        if self.def.is_some() {
            self.rewrites.push((span, text));
        }
    }

    /// The enumeration values an enum type takes: the union over its kinds.
    fn enum_values(&self, e: &EnumTy) -> Vec<String> {
        let s = self.ctx.schema;
        let mut out: Vec<String> = Vec::new();
        let kinds: Vec<Option<usize>> = if e.kinds.is_empty() {
            vec![None]
        } else {
            e.kinds.iter().map(Some).collect()
        };
        for k in kinds {
            for v in s.enum_values(k, &e.field) {
                if !out.contains(&v.name) {
                    out.push(v.name.clone());
                }
            }
        }
        out
    }

    /// The declared value a word names: exact, else the unique ASCII case-insensitive match.
    fn enum_lookup(&self, e: &EnumTy, word: &str) -> Option<String> {
        let values = self.enum_values(e);
        if values.iter().any(|v| v == word) {
            return Some(word.to_string());
        }
        let ci: Vec<&String> = values
            .iter()
            .filter(|v| v.eq_ignore_ascii_case(word))
            .collect();
        (ci.len() == 1).then(|| ci[0].clone())
    }

    /// A priority word or number: `P<n>` or `n` naming a declared value `P<n>` ([F08 §8.4.4]).
    fn priority_of(&self, e: &EnumTy, word: &str) -> Option<i64> {
        let digits = word.strip_prefix(['P', 'p']).unwrap_or(word);
        let n = i64::try_from(parse_decimal(digits)?).ok()?;
        self.enum_lookup(e, &format!("P{n}")).map(|_| n)
    }

    fn enum_kinds_text(&self, e: &EnumTy) -> String {
        self.ctx.schema.kinds_text(e.kinds)
    }

    /// E102 or E108 for a value that is not one of the enumeration's: `shown` is the value as the text prints it (a
    /// string in quotes), `raw` the value itself, which the did-you-mean candidates are measured against.
    fn not_a_value(&mut self, e: &EnumTy, shown: &str, raw: &str, span: Span, bare: bool) {
        let values = self.enum_values(e);
        let s = near(raw, values.iter().map(String::as_str));
        let mut d = if bare {
            Diag::new(
                Code::E108,
                span,
                format!("bare word {} is not a value of {}", q(shown), q(&e.field)),
            )
        } else {
            Diag::new(
                Code::E102,
                span,
                format!(
                    "{} is not a value of {} ({})",
                    q(shown),
                    q(&e.field),
                    self.enum_kinds_text(e)
                ),
            )
        };
        if let Some(first) = s.first() {
            d = d.inline(format!("did you mean {}?", q(first)));
        }
        d.suggest = s.into();
        d = d.help(format!("{} values: {}", e.field, list(&values)));
        self.err(d);
    }

    // ----- nodes ---------------------------------------------------------------------------------------------------

    /// `#N` → `NODE` (E111 for a number never allocated); in a definition, rewritten to `#u:` (§8.1).
    pub(super) fn node_num(&mut self, n: u32, span: Span, rewrite: bool) -> (CExpr, Ty) {
        match self.ctx.ids.uid(n) {
            Some(u) => {
                if rewrite {
                    self.rewrite(span, format!("#u:{}", hex(&u)));
                }
                (CExpr::Node(u), Ty::Node(self.kind_of_uid(&u)))
            }
            None => {
                let next = self.ctx.ids.next_id();
                self.err(
                    Diag::new(
                        Code::E111,
                        span,
                        format!("#{n} was never allocated in this store"),
                    )
                    .help(format!("next id is #{next}")),
                );
                (CExpr::Null, Ty::Node(self.all_kinds()))
            }
        }
    }

    /// `#u:` → `NODE` (E111 for a uid the store does not know).
    pub(super) fn node_uid(&mut self, h: &str, span: Span) -> (CExpr, Ty) {
        match parse_uid(h) {
            Some(u) if self.ctx.ids.nid(&u).is_some() => {
                (CExpr::Node(u), Ty::Node(self.kind_of_uid(&u)))
            }
            _ => {
                self.err(Diag::new(
                    Code::E111,
                    span,
                    format!("uid #u:{h} is not in this store"),
                ));
                (CExpr::Null, Ty::Node(self.all_kinds()))
            }
        }
    }

    fn kind_of_uid(&self, u: &Uid) -> KindSet {
        self.ctx
            .ids
            .kind_of(u)
            .and_then(|k| self.ctx.schema.kind(k))
            .map_or(self.all_kinds(), KindSet::one)
    }

    // ----- literals and bare words ---------------------------------------------------------------------------------

    /// An integer literal typed by its use site: a priority, a sequence number, a node, or an `INT`.
    pub(super) fn coerce_int(&mut self, n: i64, span: Span, want: Option<&Ty>) -> (CExpr, Ty) {
        match want {
            Some(Ty::Enum(e)) if e.priority => match self.priority_of(e, &n.to_string()) {
                Some(p) => (CExpr::Int(p), Ty::Enum(e.clone())),
                None => {
                    let n = n.to_string();
                    self.not_a_value(e, &n, &n, span, false);
                    (CExpr::Null, Ty::Any)
                }
            },
            Some(Ty::Rev) => match self.seq_commit(n as u64, span) {
                Some(id) => {
                    self.rewrite(span, printer::string_lit(&format!("c{}", hex(&id))));
                    (CExpr::RCommit(id), Ty::Rev)
                }
                None => (CExpr::Null, Ty::Rev),
            },
            Some(Ty::Node(_)) => match u32::try_from(n) {
                Ok(m) if m >= 1 => self.node_num(m, span, true),
                _ => {
                    self.err(Diag::new(
                        Code::E111,
                        span,
                        format!("#{n} was never allocated in this store"),
                    ));
                    (CExpr::Null, Ty::Node(self.all_kinds()))
                }
            },
            _ => (CExpr::Int(n), Ty::Int),
        }
    }

    /// A string literal typed by its use site ([LQ/canonical-ast §5.5]).
    pub(super) fn coerce_str(&mut self, s: &str, span: Span, want: Option<&Ty>) -> (CExpr, Ty) {
        match want {
            Some(Ty::Enum(e)) => {
                if e.priority {
                    if let Some(p) = self.priority_of(e, s) {
                        return (CExpr::Int(p), Ty::Enum(e.clone()));
                    }
                } else if let Some(v) = self.enum_lookup(e, s) {
                    return (CExpr::Enum(v), Ty::Enum(e.clone()));
                }
                self.not_a_value(e, &printer::string_lit(s), s, span, false);
                (CExpr::Null, Ty::Any)
            }
            Some(Ty::Rev) => match self.rev_text(s, span, true) {
                Some(lit) => {
                    // §8.1 step 4: every store-local base of the literal — each end of a range, each element of a
                    // list — becomes its full id inside the one replacement literal; suffixes and the rest stay.
                    if !lit.local.is_empty() {
                        let mut text = String::with_capacity(s.len() + 64 * lit.local.len());
                        let mut at = 0;
                        for (a, b, id) in &lit.local {
                            text.push_str(&s[at..*a]);
                            text.push('c');
                            text.push_str(&hex(id));
                            at = *b;
                        }
                        text.push_str(&s[at..]);
                        self.rewrite(span, printer::string_lit(&text));
                    }
                    (lit.c, Ty::Rev)
                }
                None => (CExpr::Text(s.to_string()), Ty::Text),
            },
            Some(Ty::Time) => match iso_datetime(s) {
                Some(t) => (CExpr::Timestamp(datetime_ms(&t)), Ty::Time),
                None => (CExpr::Text(s.to_string()), Ty::Text),
            },
            _ => (CExpr::Text(s.to_string()), Ty::Text),
        }
    }

    /// An identifier that names no binding: a bare word the use site coerces (an enum value, a priority, a revision),
    /// else E116 for a step variable seen outside its group, else a bind error ([LQ/canonical-ast §5.5]).
    pub(super) fn coerce_word(&mut self, w: &str, span: Span, want: Option<&Ty>) -> (CExpr, Ty) {
        if self.steps.iter().any(|s| s == w) {
            self.err(
                Diag::new(
                    Code::E116,
                    span,
                    format!(
                        "{} is a step variable of a quantified group and is not visible outside it",
                        q(w)
                    ),
                )
                .help("bind the endpoints outside the group: (x)((a)-[...]->(b)){m,n}(y)"),
            );
            return (CExpr::Null, Ty::Any);
        }
        match want {
            Some(Ty::Enum(e)) => {
                if e.priority {
                    if let Some(p) = self.priority_of(e, w) {
                        return (CExpr::Int(p), Ty::Enum(e.clone()));
                    }
                } else if let Some(v) = self.enum_lookup(e, w) {
                    return (CExpr::Enum(v), Ty::Enum(e.clone()));
                }
                self.not_a_value(e, w, w, span, true);
                (CExpr::Null, Ty::Any)
            }
            Some(Ty::Rev) => match self.rev_text(w, span, false) {
                Some(lit) => {
                    // A bare word is one base without suffixes: `s12` → `'c<64 hex>'` (§8.1 step 4).
                    if let Some((_, _, id)) = lit.local.first() {
                        self.rewrite(span, printer::string_lit(&format!("c{}", hex(id))));
                    }
                    (lit.c, Ty::Rev)
                }
                None => self.unbound(w, span),
            },
            _ => self.unbound(w, span),
        }
    }

    pub(super) fn unbound(&mut self, w: &str, span: Span) -> (CExpr, Ty) {
        let visible: Vec<String> = self.scope.iter().map(|(n, _)| n.clone()).collect();
        let s = near(w, visible.iter().map(String::as_str));
        let mut d = Diag::new(
            Code::E108,
            span,
            format!("bare word {} is not a bound variable or a value", q(w)),
        );
        if let Some(first) = s.first() {
            d = d.inline(format!("did you mean {}?", q(first)));
        }
        d.suggest = s.into();
        self.err(d);
        (CExpr::Null, Ty::Any)
    }

    // ----- parameters ----------------------------------------------------------------------------------------------

    /// A `$param`: inside a definition `PARAM(i)` typed by its declaration; elsewhere the bound value converted by the
    /// use site's type ([LQ/canonical-ast §5.5] "Parameters").
    pub(super) fn param(&mut self, name: &str, span: Span, want: Option<&Ty>) -> (CExpr, Ty) {
        if let Some(def) = &self.def {
            return match def.params.iter().position(|(n, _)| n == name) {
                Some(i) => (CExpr::Param(i as u32), def.params[i].1.clone()),
                None => {
                    let query = def.name.clone();
                    self.err(Diag::new(
                        Code::E110,
                        span,
                        format!("{} has no parameter {}", q(&query), q(&format!("${name}"))),
                    ));
                    (CExpr::Null, Ty::Any)
                }
            };
        }
        match self.ctx.params.get(name).cloned() {
            Some(v) => self.convert(&v, want, name, span),
            None => {
                self.err(Diag::new(
                    Code::E110,
                    span,
                    format!("{} needs {}", q("the query"), q(&format!("${name}"))),
                ));
                (CExpr::Null, Ty::Any)
            }
        }
    }

    fn bad_value(&mut self, name: &str, ty: &Ty, v: &Value, span: Span) -> (CExpr, Ty) {
        let t = match ty {
            Ty::Enum(e) => e.field.clone(),
            other => other.name(),
        };
        self.err(Diag::new(
            Code::E110,
            span,
            format!(
                "{} must be {t}; got {}",
                q(&format!("${name}")),
                q(&cap(&value_text(v), 64))
            ),
        ));
        (CExpr::Null, Ty::Any)
    }

    /// Converts a bound value to the constant of its use-site type ([LQ/canonical-ast §5.5], [LQ/std §2.2]).
    pub(super) fn convert(
        &mut self,
        v: &Value,
        want: Option<&Ty>,
        name: &str,
        span: Span,
    ) -> (CExpr, Ty) {
        if *v == Value::Null {
            return (CExpr::Null, Ty::Null);
        }
        let Some(want) = want.filter(|w| !matches!(w, Ty::Any | Ty::Null | Ty::Map)) else {
            return match v {
                Value::Bool(b) => (CExpr::Bool(*b), Ty::Bool),
                Value::Int(n) => (CExpr::Int(*n), Ty::Int),
                Value::Float(f) => self.float_value(*f, name, v, span),
                Value::Text(t) => (CExpr::Text(t.clone()), Ty::Text),
                Value::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    let mut elem = Ty::Any;
                    for x in items {
                        let (c, t) = self.convert(x, None, name, span);
                        if elem == Ty::Any {
                            elem = t;
                        }
                        out.push(c);
                    }
                    (CExpr::List(out), Ty::List(Box::new(elem)))
                }
                Value::Null => (CExpr::Null, Ty::Null),
            };
        };
        match (want, v) {
            (Ty::Node(_), Value::Int(n)) => match u32::try_from(*n) {
                Ok(m) if m >= 1 => self.node_num(m, span, false),
                _ => self.bad_value(name, want, v, span),
            },
            (Ty::Node(_), Value::Text(t)) => match node_text(t) {
                Some(NodeRef::Num(n)) => self.node_num(n, span, false),
                Some(NodeRef::Uid(u)) => self.node_uid(&hex(&u), span),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::List(elem), Value::List(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(self.convert(x, Some(elem), name, span).0);
                }
                if **elem == Ty::Rev {
                    (CExpr::RList(out), want.clone())
                } else {
                    (CExpr::List(out), want.clone())
                }
            }
            (Ty::List(elem), Value::Text(t)) => {
                let intlike =
                    matches!(**elem, Ty::Int) || matches!(&**elem, Ty::Enum(e) if e.priority);
                if intlike && t.contains("..") {
                    return match range_text(t) {
                        Some((lo, hi)) => (CExpr::RangeInt(lo, hi), Ty::Range),
                        None => self.bad_value(name, want, v, span),
                    };
                }
                let mut out = Vec::new();
                for piece in t.split(',').filter(|p| !p.is_empty()) {
                    out.push(
                        self.convert(&Value::Text(piece.to_string()), Some(elem), name, span)
                            .0,
                    );
                }
                if **elem == Ty::Rev {
                    (CExpr::RList(out), want.clone())
                } else {
                    (CExpr::List(out), want.clone())
                }
            }
            (Ty::List(elem), Value::Int(_)) => {
                let (c, _) = self.convert(v, Some(elem), name, span);
                (CExpr::List(vec![c]), want.clone())
            }
            (Ty::Range, Value::Text(t)) => match range_text(t) {
                Some((lo, hi)) => (CExpr::RangeInt(lo, hi), Ty::Range),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::Range, Value::Int(n)) => (CExpr::RangeInt(Some(*n), Some(*n)), Ty::Range),
            (Ty::Int, Value::Int(n)) => (CExpr::Int(*n), Ty::Int),
            (Ty::Int, Value::Text(t)) => {
                let (neg, d) = t
                    .strip_prefix('-')
                    .map_or((false, t.as_str()), |d| (true, d));
                match parse_decimal(d).and_then(|n| i64::try_from(n).ok()) {
                    Some(n) => (CExpr::Int(if neg { -n } else { n }), Ty::Int),
                    None => self.bad_value(name, want, v, span),
                }
            }
            (Ty::Float, Value::Float(f)) => self.float_value(*f, name, v, span),
            (Ty::Float, Value::Int(n)) => (CExpr::Float(*n as f64), Ty::Float),
            (Ty::Float, Value::Text(t)) => match t.parse::<f64>() {
                Ok(f) => self.float_value(f, name, v, span),
                Err(_) => self.bad_value(name, want, v, span),
            },
            (Ty::Bool, Value::Bool(b)) => (CExpr::Bool(*b), Ty::Bool),
            (Ty::Bool, Value::Text(t)) if t == "true" || t == "false" => {
                (CExpr::Bool(t == "true"), Ty::Bool)
            }
            (Ty::Text | Ty::KindName, Value::Text(t)) => (CExpr::Text(t.clone()), Ty::Text),
            (Ty::Enum(e), Value::Text(t)) => {
                if e.priority {
                    match self.priority_of(e, t) {
                        Some(p) => (CExpr::Int(p), want.clone()),
                        None => self.bad_value(name, want, v, span),
                    }
                } else {
                    match self.enum_lookup(e, t) {
                        Some(x) => (CExpr::Enum(x), want.clone()),
                        None => self.bad_value(name, want, v, span),
                    }
                }
            }
            (Ty::Enum(e), Value::Int(n)) if e.priority => match self.priority_of(e, &n.to_string())
            {
                Some(p) => (CExpr::Int(p), want.clone()),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::Rev, Value::Text(t)) => match self.rev_text(t, span, true) {
                Some(lit) => (lit.c, Ty::Rev),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::Rev, Value::Int(n)) => match self.seq_commit(*n as u64, span) {
                Some(id) => (CExpr::RCommit(id), Ty::Rev),
                None => (CExpr::Null, Ty::Rev),
            },
            (Ty::Time, Value::Text(t)) => match iso_datetime(t) {
                Some(n) => (CExpr::Timestamp(datetime_ms(&n)), Ty::Time),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::Time, Value::Int(n)) => (CExpr::Timestamp(*n), Ty::Time),
            (Ty::Dur, Value::Text(t)) => match duration_ms(t) {
                Some(ms) => (CExpr::Duration(ms), Ty::Dur),
                None => self.bad_value(name, want, v, span),
            },
            (Ty::Dur, Value::Int(n)) => (CExpr::Duration(*n), Ty::Dur),
            _ => self.bad_value(name, want, v, span),
        }
    }

    /// `FLOAT`: −0.0 is encoded as +0.0 and a NaN is refused (E110, [LQ/canonical-ast §5.5]).
    fn float_value(&mut self, f: f64, name: &str, v: &Value, span: Span) -> (CExpr, Ty) {
        if f.is_nan() || f.is_infinite() {
            return self.bad_value(name, &Ty::Float, v, span);
        }
        (CExpr::Float(if f == 0.0 { 0.0 } else { f }), Ty::Float)
    }

    // ----- revisions -----------------------------------------------------------------------------------------------

    /// The commit with store sequence number `n` (E301 if none).
    fn seq_commit(&mut self, n: u64, span: Span) -> Option<CommitId> {
        let c = self.ctx.ids.commit_by_seq(n);
        if c.is_none() {
            self.err(Diag::new(
                Code::E301,
                span,
                format!("unknown revision {}", q(&format!("s{n}"))),
            ));
        }
        c
    }

    /// The unique commit whose id starts with `h`, E301 if none or several ([LQ/canonical-ast §5.6] `rcommit`). A
    /// literal of all 64 hex digits binds as that id whether or not the store holds the commit (spec sync 2a; E301 is
    /// raised only when a view of it is resolved; [LQ/lexical §10.2]).
    fn commit_prefix(&mut self, h: &str, span: Span) -> Option<CommitId> {
        let found = self.ctx.ids.commits_by_prefix(h);
        if found.is_empty()
            && let Some(id) = unhex::<32>(&h.to_ascii_lowercase())
        {
            return Some(id);
        }
        match found.len() {
            1 => Some(found[0].0),
            0 => {
                self.err(Diag::new(
                    Code::E301,
                    span,
                    format!("unknown revision {}", q(&format!("c{h}"))),
                ));
                None
            }
            n => {
                let mut d = Diag::new(
                    Code::E301,
                    span,
                    format!("{} matches {n} commits", q(&format!("c{h}"))),
                )
                .help("write more hex digits");
                for (id, seq, r) in found.iter().take(5) {
                    d = d.detail(format!("c{} rev {seq} {r}", &hex(id)[..8]));
                }
                self.err(d);
                None
            }
        }
    }

    /// A revision read in revision mode ([LQ/canonical-ast §5.6]).
    pub(super) fn rev(&mut self, r: &Rev) -> CExpr {
        self.rev_in(r, None)
    }

    /// A revision; `at` is the span of the literal it was read from (a coerced string or word), else `None`. Suffixes
    /// nest left to right and a revision may carry any number of them, so the chain is walked in a loop
    /// ([LQ/grammar-v1.ebnf §P.13]).
    fn rev_in(&mut self, r: &Rev, at: Option<Span>) -> CExpr {
        let span = at.unwrap_or(r.span);
        let mut sufs: Vec<&Suffix> = Vec::new();
        let mut base = r;
        while let RevKind::Suf(b, s) = &base.kind {
            sufs.push(s);
            base = b;
        }
        let mut c = match &base.kind {
            RevKind::Head => CExpr::RHead,
            RevKind::Ref(n) => CExpr::RRef(n.clone()),
            RevKind::Commit(h) => match self.commit_prefix(h, span) {
                Some(id) => {
                    if at.is_none() && h.len() < 64 {
                        self.rewrite(base.span, format!("c{}", hex(&id)));
                    }
                    CExpr::RCommit(id)
                }
                None => CExpr::Null,
            },
            RevKind::Seq(n) => match self.seq_commit(*n, span) {
                Some(id) => {
                    if at.is_none() {
                        self.rewrite(base.span, format!("c{}", hex(&id)));
                    }
                    CExpr::RCommit(id)
                }
                None => CExpr::Null,
            },
            RevKind::Param(p) => {
                let (c, t) = self.param(p, span, Some(&Ty::Rev));
                if !matches!(&t, Ty::Rev | Ty::Any | Ty::Null) && t != Ty::List(Box::new(Ty::Rev)) {
                    self.err(Diag::new(
                        Code::E103,
                        span,
                        format!(
                            "{} is {}, not a revision",
                            q(&format!("${p}")),
                            q(&t.name())
                        ),
                    ));
                }
                c
            }
            RevKind::Suf(..) => unreachable!("the loop above walked every suffix"),
        };
        for suf in sufs.into_iter().rev() {
            let (kind, n) = match suf {
                Suffix::Tilde(n) => (1, i64::from(*n)),
                Suffix::Caret(n) => (2, i64::from(*n)),
                Suffix::At(n) => (3, i64::from(*n)),
                Suffix::AtTime(t) => (4, datetime_ms(t)),
            };
            // A reflog suffix is store-local: E117 inside a definition (§8.1 step 5).
            if kind >= 3 && self.def.is_some() {
                self.anchor_handle(span);
            }
            c = CExpr::RSuf(Box::new(c), kind, n);
        }
        c
    }

    /// A revision argument in revision mode: a revspec, a range or a list.
    pub(super) fn rev_arg(&mut self, a: &ArgVal) -> CExpr {
        match a {
            ArgVal::Rev(r) => self.rev(r),
            ArgVal::Range { from, op, to, .. } => {
                let f = self.rev(from);
                let t = self.rev(to);
                CExpr::RRange(
                    Box::new(f),
                    if *op == RangeOp::Two { 1 } else { 2 },
                    Box::new(t),
                )
            }
            ArgVal::List(v, _) => CExpr::RList(v.iter().map(|r| self.rev(r)).collect()),
            ArgVal::Expr(e) => self.expr(e, Some(&Ty::Rev)).0,
        }
    }

    /// Reads a text with the revision grammar ([LQ/lexical §7]); `arg` admits ranges and lists. `None` when the text
    /// is not of revision shape.
    pub(super) fn rev_text(&mut self, text: &str, span: Span, arg: bool) -> Option<RevLit> {
        let lx = Lexer::new(text);
        let (read, end) = lx.revision(0, arg, &mut Vec::new()).ok()?;
        if lx.skip_trivia(end).ok()? != text.len() {
            return None;
        }
        let mut local = Vec::new();
        let mut one = |b: &mut Self, r: &Rev| -> CExpr {
            let c = b.rev_in(r, Some(span));
            if short_base(r)
                && let Some(id) = first_commit(&c)
            {
                let s = base_of(r).span;
                local.push((s.start as usize, s.end as usize, id));
            }
            c
        };
        let (c, first) = match read {
            RevRead::Rev(r) => (one(self, &r), r),
            RevRead::Range(a, op, b, _) => {
                let x = one(self, &a);
                let y = one(self, &b);
                let op = if op == RangeOp::Two { 1 } else { 2 };
                (CExpr::RRange(Box::new(x), op, Box::new(y)), a)
            }
            RevRead::List(v, _) => {
                let items = v.iter().map(|r| one(self, r)).collect();
                (CExpr::RList(items), v.into_iter().next()?)
            }
            RevRead::Quote => return None,
        };
        Some(RevLit { c, first, local })
    }
}

/// A revision literal read from a text: a coerced string or bare word, or a `rev` parameter's value
/// ([LQ/canonical-ast §5.6], last row).
pub(super) struct RevLit {
    /// The constant.
    pub c: CExpr,
    /// The first revspec read (the view of a `USE $param`, [50 §3.9] item 6).
    pub first: Rev,
    /// Each store-local base (a sequence number or a commit prefix, §8.1 step 4) with the full id it resolved to: the
    /// base's byte range in the text, in text order — both ends of a range, every element of a list.
    pub local: Vec<(usize, usize, CommitId)>,
}

/// The full commit id at the base of a revision constant, if it resolved to one.
fn first_commit(c: &CExpr) -> Option<CommitId> {
    let mut c = c;
    loop {
        match c {
            CExpr::RCommit(id) => return Some(*id),
            CExpr::RSuf(b, _, _) => c = b,
            _ => return None,
        }
    }
}
