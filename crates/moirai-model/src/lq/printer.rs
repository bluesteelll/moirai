//! The display printer of [LQ/gql-spelling §5]: LQ text from an S-AST, in either display spelling of quantifiers
//! (§4, `HOLE(LQ-display-spelling)`).
//!
//! It keeps the names the text used, prints keywords in upper case, back-quotes every name that is not a plain word or
//! that is reserved where a reserved word would be read as a keyword, and prints strings in single quotes with `\`,
//! `'`, LF, CR, HT and the refused controls escaped. The printer property of [LQ/canonical-ast §3.4] holds: for every
//! S-AST `a` the parser can build, `parse(print(a, s)) == a` for both spellings `s`.

use crate::lq::ast::*;
use crate::lq::parser::is_reserved;

/// The display spelling of quantifiers ([LQ/gql-spelling §4.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Spelling {
    /// `*1..`, `*2..`, `*m..n` inside the brackets (the default of [LQ/card §3]).
    #[default]
    Cypher,
    /// `+`, `*`, `{m,}`, `{m}`, `{m,n}` after the edge.
    Gql,
}

fn is_plain_word(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

fn backquote(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// A name in a plain-name position ([LQ/lexical §6.3]): as written when it is a word, else back-quoted.
pub fn plain(s: &str) -> String {
    if is_plain_word(s) {
        s.to_string()
    } else {
        backquote(s)
    }
}

/// A variable, alias or target: back-quoted when it is not a word, when it is reserved, or when it is `ALL` (a
/// contextual keyword right after `RETURN`).
pub fn var(s: &str) -> String {
    if is_plain_word(s) && !is_reserved(s) && !s.eq_ignore_ascii_case("all") {
        s.to_string()
    } else {
        backquote(s)
    }
}

/// A generic function name: back-quoted when unquoted it would be a keyword form or a refused name
/// ([LQ/grammar-v1.ebnf §P.9], §R).
fn fn_name(s: &str) -> String {
    const SPECIAL: [&str; 12] = [
        "all",
        "any",
        "none",
        "exists",
        "size",
        "shortestpath",
        "allshortestpaths",
        "nodes",
        "relationships",
        "single",
        "timestamp",
        "cast",
    ];
    let l = s.to_ascii_lowercase();
    if is_plain_word(s) && !is_reserved(s) && !SPECIAL.contains(&l.as_str()) {
        s.to_string()
    } else {
        backquote(s)
    }
}

/// A procedure name: segments split at `.`; a first segment that would be refused or read as `tx` is back-quoted.
fn proc_name(s: &str) -> String {
    let mut out = Vec::new();
    for (i, seg) in s.split('.').enumerate() {
        let l = seg.to_ascii_lowercase();
        if i == 0 && ["apoc", "gds", "db", "dbms", "tx"].contains(&l.as_str()) {
            out.push(backquote(seg));
        } else {
            out.push(plain(seg));
        }
    }
    out.join(".")
}

/// A string literal in single quotes ([LQ/canonical-ast §3.4]).
pub fn string_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if matches!(c as u32, 0x00..=0x1F | 0x7F) => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// The GQL form of a quantifier: `+`, `*`, `{m,}`, `{m}`, `{m,n}`.
pub fn gql_quant(q: Quant) -> String {
    match (q.min, q.max) {
        (1, None) => "+".into(),
        (0, None) => "*".into(),
        (m, None) => format!("{{{m},}}"),
        (m, Some(n)) if m == n => format!("{{{m}}}"),
        (m, Some(n)) => format!("{{{m},{n}}}"),
    }
}

/// The Cypher form of a quantifier, written inside the brackets: `*1..`, `*0..`, `*m..`, `*m`, `*m..n`.
pub fn cypher_quant(q: Quant) -> String {
    match (q.min, q.max) {
        (m, None) => format!("*{m}.."),
        (m, Some(n)) if m == n => format!("*{m}"),
        (m, Some(n)) => format!("*{m}..{n}"),
    }
}

/// The text of an expression in the default display spelling.
pub fn expr_text(e: &Expr) -> String {
    Printer::new(Spelling::default()).expr(e, 0)
}

/// The text of a pattern path in the default display spelling.
pub fn path_text(p: &Path) -> String {
    Printer::new(Spelling::default()).path(p)
}

/// The text of one edge pattern in a display spelling.
pub fn edge_text(e: &EPat, spelling: Spelling) -> String {
    Printer::new(spelling).epat(e)
}

/// The text of a projection list (`*, a AS b, ...`).
pub fn proj_items_text(star: bool, items: &[Item]) -> String {
    Printer::new(Spelling::default()).proj(star, items)
}

/// The text of one sort key.
pub fn sort_text(s: &Sort) -> String {
    Printer::new(Spelling::default()).sort(s)
}

/// Prints a read root.
pub fn print_read(r: &Read, spelling: Spelling) -> String {
    let p = Printer::new(spelling);
    let mut lines = Vec::new();
    match r.mode {
        Mode::Run => {}
        Mode::Explain => lines.push("EXPLAIN".to_string()),
        Mode::Profile => lines.push("PROFILE".to_string()),
    }
    p.query_lines(&r.query, &mut lines, "");
    lines.join("\n")
}

/// Prints a write root.
pub fn print_tx(t: &Tx, spelling: Spelling) -> String {
    Printer::new(spelling).tx(t)
}

/// Prints a definition in the layout of [LQ/gql-spelling §5.2].
pub fn print_define(d: &Define, spelling: Spelling) -> String {
    let p = Printer::new(spelling);
    let mut lines = Vec::new();
    lines.push(format!("{} {{", p.define_header(d, true)));
    p.query_lines(&d.body, &mut lines, "  ");
    lines.push("}".to_string());
    lines.join("\n")
}

/// Prints a query on one line.
pub fn print_query_inline(q: &Query, spelling: Spelling) -> String {
    Printer::new(spelling).query_inline(q)
}

/// The display printer.
#[derive(Clone, Copy)]
pub struct Printer {
    spelling: Spelling,
}

impl Printer {
    /// A printer in a display spelling.
    pub fn new(spelling: Spelling) -> Printer {
        Printer { spelling }
    }

    // ----- queries -------------------------------------------------------------------------------------------------

    fn query_lines(&self, q: &Query, lines: &mut Vec<String>, indent: &str) {
        for (i, part) in q.parts.iter().enumerate() {
            if i > 0 {
                let op = match q.ops[i - 1] {
                    SetOp::Union => "UNION",
                    SetOp::UnionAll => "UNION ALL",
                    SetOp::Except => "EXCEPT",
                    SetOp::Intersect => "INTERSECT",
                };
                lines.push(format!("{indent}{op}"));
            }
            for l in self.part_lines(part) {
                lines.push(format!("{indent}{l}"));
            }
        }
    }

    /// A query on one line (subqueries, `RESOLVE`, statements of a `TX`).
    pub fn query_inline(&self, q: &Query) -> String {
        let mut out = Vec::new();
        for (i, part) in q.parts.iter().enumerate() {
            if i > 0 {
                out.push(
                    match q.ops[i - 1] {
                        SetOp::Union => "UNION",
                        SetOp::UnionAll => "UNION ALL",
                        SetOp::Except => "EXCEPT",
                        SetOp::Intersect => "INTERSECT",
                    }
                    .to_string(),
                );
            }
            out.push(self.part_lines(part).join(" "));
        }
        out.join(" ")
    }

    fn part_lines(&self, part: &Part) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(u) = &part.use_ {
            lines.push(format!("USE {}", self.rev(u)));
        }
        match &part.body {
            PartBody::Call(c) => {
                let mut head = format!("CALL {}({})", proc_name(&c.proc.text), self.args(&c.args));
                match &c.yield_ {
                    YieldMode::None => {}
                    YieldMode::Star => head.push_str(" YIELD *"),
                    YieldMode::Items(items) => {
                        head.push_str(" YIELD ");
                        head.push_str(&self.yields(items));
                    }
                }
                lines.push(head);
                if let Some(w) = &c.where_ {
                    self.where_lines(w, &mut lines);
                }
                let tail = self.order_limit(&c.order, &c.limit);
                if !tail.is_empty() {
                    let last = lines.last_mut().expect("the CALL line exists");
                    last.push(' ');
                    last.push_str(&tail);
                }
            }
            PartBody::Clauses { clauses, ret } => {
                for c in clauses {
                    self.clause_lines(c, &mut lines);
                }
                lines.push(self.ret(ret));
            }
        }
        lines
    }

    fn where_lines(&self, w: &Expr, lines: &mut Vec<String>) {
        let mut conj = Vec::new();
        let mut cur = w;
        while let ExprKind::And(l, r) = &cur.kind {
            conj.push(r.as_ref());
            cur = l;
        }
        conj.push(cur);
        conj.reverse();
        lines.push(format!("WHERE {}", self.expr(conj[0], 3)));
        for c in &conj[1..] {
            lines.push(format!("  AND {}", self.expr(c, 3)));
        }
    }

    fn clause_lines(&self, c: &Clause, lines: &mut Vec<String>) {
        match c {
            Clause::Match(m) => {
                lines.push(self.match_head(m));
                if let Some(w) = &m.where_ {
                    self.where_lines(w, lines);
                }
            }
            Clause::Call(c) => {
                lines.push(format!(
                    "CALL {}({}) YIELD {}",
                    proc_name(&c.proc.text),
                    self.args(&c.args),
                    self.yields(&c.yield_)
                ));
                if let Some(w) = &c.where_ {
                    self.where_lines(w, lines);
                }
            }
            Clause::Unwind(u) => lines.push(format!(
                "UNWIND {} AS {}",
                self.expr(&u.expr, 0),
                var(&u.as_.text)
            )),
            Clause::With(w) => {
                lines.push(format!(
                    "WITH {}{}",
                    if w.distinct { "DISTINCT " } else { "" },
                    self.proj(w.star, &w.items)
                ));
                if let Some(e) = &w.where_ {
                    self.where_lines(e, lines);
                }
                let tail = self.order_limit(&w.order, &w.limit);
                if !tail.is_empty() {
                    let last = lines.last_mut().expect("the WITH line exists");
                    last.push(' ');
                    last.push_str(&tail);
                }
            }
        }
    }

    fn clause_inline(&self, c: &Clause) -> String {
        let mut lines = Vec::new();
        self.clause_lines(c, &mut lines);
        lines
            .iter()
            .map(|l| l.trim_start())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn match_head(&self, m: &Match) -> String {
        let mut s = String::from(if m.optional {
            "OPTIONAL MATCH "
        } else {
            "MATCH "
        });
        if let Some(mode) = m.mode {
            s.push_str(match mode {
                MatchMode::Walk => "WALK ",
                MatchMode::Trail => "TRAIL ",
                MatchMode::Acyclic => "ACYCLIC ",
                MatchMode::Simple => "SIMPLE ",
                MatchMode::Different => "DIFFERENT EDGES ",
            });
        }
        s.push_str(&self.patterns(&m.patterns));
        s
    }

    fn ret(&self, r: &Return) -> String {
        let mut s = format!(
            "RETURN {}{}",
            if r.distinct { "DISTINCT " } else { "" },
            self.proj(r.star, &r.items)
        );
        if !r.group.is_empty() {
            s.push_str(" GROUP BY ");
            s.push_str(
                &r.group
                    .iter()
                    .map(|e| self.expr(e, 0))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        let tail = self.order_limit(&r.order, &r.limit);
        if !tail.is_empty() {
            s.push(' ');
            s.push_str(&tail);
        }
        s
    }

    fn proj(&self, star: bool, items: &[Item]) -> String {
        let mut parts = Vec::new();
        if star {
            parts.push("*".to_string());
        }
        for it in items {
            let mut s = self.expr(&it.expr, 0);
            if let Some(a) = &it.as_ {
                s.push_str(" AS ");
                s.push_str(&var(&a.text));
            }
            parts.push(s);
        }
        parts.join(", ")
    }

    fn sort(&self, s: &Sort) -> String {
        let mut t = self.expr(&s.expr, 0);
        if s.desc {
            t.push_str(" DESC");
        }
        t
    }

    fn order_limit(&self, order: &[Sort], limit: &Option<Expr>) -> String {
        let mut parts = Vec::new();
        if !order.is_empty() {
            parts.push(format!(
                "ORDER BY {}",
                order
                    .iter()
                    .map(|s| self.sort(s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(l) = limit {
            parts.push(format!("LIMIT {}", self.expr(l, 0)));
        }
        parts.join(" ")
    }

    fn yields(&self, items: &[YItem]) -> String {
        items
            .iter()
            .map(|y| match &y.as_ {
                Some(a) => format!("{} AS {}", plain(&y.name.text), var(&a.text)),
                None => plain(&y.name.text),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn args(&self, args: &[Arg]) -> String {
        args.iter()
            .map(|a| self.arg(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn arg(&self, a: &Arg) -> String {
        let v = match &a.value {
            ArgVal::Expr(e) => {
                let t = self.expr(e, 0);
                if a.name.is_none() && starts_with_word_colon(e) {
                    format!("({t})")
                } else {
                    t
                }
            }
            ArgVal::Rev(r) => self.rev(r),
            ArgVal::Range { from, op, to, .. } => {
                format!(
                    "{}{}{}",
                    self.rev(from),
                    if *op == RangeOp::Two { ".." } else { "..." },
                    self.rev(to)
                )
            }
            ArgVal::List(elems, _) => format!(
                "[{}]",
                elems
                    .iter()
                    .map(|r| self.rev(r))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        match &a.name {
            Some(n) => format!("{}: {v}", plain(&n.text)),
            None => v,
        }
    }

    /// A revision as written ([LQ/lexical §7]); its suffix chain is walked in a loop.
    pub fn rev(&self, r: &Rev) -> String {
        let mut sufs = Vec::new();
        let mut base = r;
        while let RevKind::Suf(b, s) = &base.kind {
            sufs.push(s);
            base = b;
        }
        let mut out = match &base.kind {
            RevKind::Head => "HEAD".to_string(),
            RevKind::Ref(n) => n.clone(),
            RevKind::Commit(h) => format!("c{h}"),
            RevKind::Seq(n) => format!("s{n}"),
            RevKind::Param(p) => format!("${p}"),
            RevKind::Suf(..) => unreachable!("the loop above walked every suffix"),
        };
        for s in sufs.into_iter().rev() {
            match s {
                Suffix::Tilde(n) => out.push_str(&format!("~{n}")),
                Suffix::Caret(n) => out.push_str(&format!("^{n}")),
                Suffix::At(n) => out.push_str(&format!("@{n}")),
                Suffix::AtTime(t) => {
                    out.push('@');
                    out.push_str(t);
                }
            }
        }
        out
    }

    // ----- patterns ------------------------------------------------------------------------------------------------

    fn patterns(&self, ps: &[Path]) -> String {
        ps.iter()
            .map(|p| self.path(p))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A pattern path.
    pub fn path(&self, p: &Path) -> String {
        let mut s = self.npat(&p.start);
        for step in &p.steps {
            match step {
                Step::Edge(e, n) => {
                    s.push_str(&self.epat(e));
                    s.push_str(&self.npat(n));
                }
                Step::Group(g, n) => {
                    s.push('(');
                    s.push_str(&self.path(&g.path));
                    if let Some(w) = &g.where_ {
                        s.push_str(" WHERE ");
                        s.push_str(&self.expr(w, 0));
                    }
                    s.push(')');
                    s.push_str(&gql_quant(g.quant));
                    s.push_str(&self.npat(n));
                }
            }
        }
        s
    }

    fn props(&self, kvs: &[Kv]) -> String {
        format!(
            "{{{}}}",
            kvs.iter()
                .map(|kv| format!("{}: {}", plain(&kv.key.text), self.expr(&kv.value, 0)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn npat(&self, n: &NPat) -> String {
        if n.var.is_none()
            && n.labels.is_empty()
            && n.where_.is_none()
            && n.props.len() == 1
            && n.props[0].key.text == "id"
        {
            match &n.props[0].value.kind {
                ExprKind::Nid(k) => return format!("(#{k})"),
                ExprKind::Uid(h) => return format!("(#u:{h})"),
                _ => {}
            }
        }
        let mut s = String::from("(");
        if let Some(v) = &n.var {
            s.push_str(&var(&v.text));
        }
        if !n.labels.is_empty() {
            s.push(':');
            s.push_str(
                &n.labels
                    .iter()
                    .map(|l| plain(&l.text))
                    .collect::<Vec<_>>()
                    .join("|"),
            );
        }
        if !n.props.is_empty() {
            if s.len() > 1 {
                s.push(' ');
            }
            s.push_str(&self.props(&n.props));
        }
        if let Some(w) = &n.where_ {
            if s.len() > 1 {
                s.push(' ');
            }
            s.push_str("WHERE ");
            s.push_str(&self.expr(w, 0));
        }
        s.push(')');
        s
    }

    fn epat(&self, e: &EPat) -> String {
        let mut body = String::new();
        if let Some(v) = &e.var {
            body.push_str(&var(&v.text));
        }
        if !e.types.is_empty() {
            body.push(':');
            body.push_str(
                &e.types
                    .iter()
                    .map(|t| plain(&t.text))
                    .collect::<Vec<_>>()
                    .join("|"),
            );
        }
        let mut post = String::new();
        if let Some(q) = e.quant {
            match self.spelling {
                Spelling::Cypher => body.push_str(&cypher_quant(q)),
                Spelling::Gql => post = gql_quant(q),
            }
        }
        if !e.props.is_empty() {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(&self.props(&e.props));
        }
        if let Some(w) = &e.where_ {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str("WHERE ");
            body.push_str(&self.expr(w, 0));
        }
        let s = match e.dir {
            Dir::Right => format!("-[{body}]->"),
            Dir::Left => format!("<-[{body}]-"),
            Dir::Both => format!("-[{body}]-"),
        };
        s + &post
    }

    // ----- expressions ---------------------------------------------------------------------------------------------

    /// An expression, parenthesised when its precedence is below `min` (levels of [LQ/grammar-v1.ebnf §4]: 1 `OR`,
    /// 2 `AND`, 3 `NOT`, 4 predicates, 5 `+ -`, 6 `* /`, 7 unary `-`, 8 postfix, 9 primary).
    pub fn expr(&self, e: &Expr, min: u8) -> String {
        let mut out = String::new();
        self.expr_into(e, min, &mut out);
        out
    }

    /// [`Printer::expr`] into `out`. An operator's text starts with its first operand ([`Expr::first_operand`]), in
    /// which left-associative chains nest, so that operand is walked in a loop ([LQ/grammar-v1.ebnf §P.13]): each
    /// node's parenthesis and prefix are written top-down, then the innermost node, then each node's operator and other
    /// operand bottom-up. Only the other operands recurse, and nesting bounds their depth.
    fn expr_into(&self, e: &Expr, min: u8, out: &mut String) {
        // (node, parenthesised, where its first operand's text starts)
        let mut spine: Vec<(&Expr, bool, usize)> = Vec::new();
        let mut cur = e;
        let mut min = min;
        while let Some(first) = cur.first_operand() {
            let paren = prec(cur) < min;
            if paren {
                out.push('(');
            }
            match &cur.kind {
                ExprKind::Not(_) => out.push_str("NOT "),
                ExprKind::Neg(_) => out.push('-'),
                _ => {}
            }
            spine.push((cur, paren, out.len()));
            min = first_min(cur);
            cur = first;
        }
        let paren = prec(cur) < min;
        if paren {
            out.push('(');
        }
        self.primary_into(cur, out);
        if paren {
            out.push(')');
        }
        while let Some((node, paren, left_at)) = spine.pop() {
            self.operator_tail(node, left_at, out);
            if paren {
                out.push(')');
            }
        }
    }

    /// What an operator writes after its first operand, whose text starts at `left_at`.
    fn operator_tail(&self, e: &Expr, left_at: usize, out: &mut String) {
        match &e.kind {
            ExprKind::Or(_, r) => {
                out.push_str(" OR ");
                self.expr_into(r, 2, out);
            }
            ExprKind::And(_, r) => {
                out.push_str(" AND ");
                self.expr_into(r, 3, out);
            }
            ExprKind::Cmp(op, _, r) => {
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                self.expr_into(r, 5, out);
            }
            ExprKind::IsNull(neg, _) => {
                out.push_str(if *neg { " IS NOT NULL" } else { " IS NULL" });
            }
            ExprKind::In(_, r) => {
                out.push_str(" IN ");
                self.expr_into(r, 5, out);
            }
            ExprKind::StrPred(op, _, r) => {
                out.push_str(match op {
                    StrOp::Starts => " STARTS WITH ",
                    StrOp::Ends => " ENDS WITH ",
                    StrOp::Contains => " CONTAINS ",
                });
                self.expr_into(r, 5, out);
            }
            ExprKind::LabelTest(_, labels) => {
                out.push(':');
                for (i, l) in labels.iter().enumerate() {
                    if i > 0 {
                        out.push('|');
                    }
                    out.push_str(&plain(&l.text));
                }
            }
            ExprKind::Arith(op, _, r) => {
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                let rm = match op {
                    ArithOp::Add | ArithOp::Sub => 6,
                    ArithOp::Mul | ArithOp::Div => 7,
                };
                let right_at = out.len();
                self.expr_into(r, rm, out);
                // `(a:b) - -(c:d)` and `(a:b) - [x]-(c:d)` would read as a path predicate (P5): keep the operand apart.
                if *op == ArithOp::Sub
                    && out[left_at..].starts_with('(')
                    && (out[right_at..].starts_with('-') || out[right_at..].starts_with('['))
                {
                    out.insert(right_at, '(');
                    out.push(')');
                }
            }
            ExprKind::Prop(_, n) => {
                out.push('.');
                out.push_str(&plain(&n.text));
            }
            // `NOT` and unary `-` wrote their prefix before the operand.
            _ => {}
        }
    }

    /// A node that is no operator (precedence 9).
    fn primary_into(&self, e: &Expr, out: &mut String) {
        match &e.kind {
            ExprKind::Ident(n) => out.push_str(&var(n)),
            ExprKind::Param(n) => {
                out.push('$');
                out.push_str(n);
            }
            ExprKind::Nid(n) => {
                out.push('#');
                out.push_str(&n.to_string());
            }
            ExprKind::Uid(h) => {
                out.push_str("#u:");
                out.push_str(h);
            }
            ExprKind::Int(n) => out.push_str(&n.to_string()),
            ExprKind::Float(t) | ExprKind::Dur(t) => out.push_str(t),
            ExprKind::Str(s) => out.push_str(&string_lit(s)),
            ExprKind::Bool(b) => out.push_str(if *b { "TRUE" } else { "FALSE" }),
            ExprKind::Null => out.push_str("NULL"),
            ExprKind::Exists(sub) => {
                out.push_str("EXISTS { ");
                out.push_str(&self.sub(sub));
                out.push_str(" }");
            }
            ExprKind::CountSub(sub) => {
                out.push_str("COUNT { ");
                out.push_str(&self.sub(sub));
                out.push_str(" }");
            }
            ExprKind::Fn {
                name,
                distinct,
                args,
            } => {
                out.push_str(&fn_name(&name.text));
                out.push('(');
                if *distinct {
                    out.push_str("DISTINCT");
                    if !args.is_empty() {
                        out.push(' ');
                    }
                }
                out.push_str(&self.args(args));
                out.push(')');
            }
            ExprKind::CountStar => out.push_str("count(*)"),
            ExprKind::ListPred {
                kind,
                var: v,
                list,
                pred,
            } => {
                out.push_str(match kind {
                    ListPredKind::All => "all(",
                    ListPredKind::Any => "any(",
                    ListPredKind::None => "none(",
                });
                out.push_str(&var(&v.text));
                out.push_str(" IN ");
                self.expr_into(list, 0, out);
                out.push_str(" WHERE ");
                self.expr_into(pred, 0, out);
                out.push(')');
            }
            ExprKind::List(elems) => {
                out.push('[');
                for (i, x) in elems.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    // `[word IN ...` is refused as a list comprehension ([LQ/grammar-v1.ebnf §R]): keep it apart.
                    if i == 0 && starts_with_word_in(x) {
                        out.push('(');
                        self.expr_into(x, 0, out);
                        out.push(')');
                    } else {
                        self.expr_into(x, 0, out);
                    }
                }
                out.push(']');
            }
            ExprKind::Map(kvs) => out.push_str(&self.props(kvs)),
            ExprKind::Case {
                subject,
                whens,
                else_,
            } => {
                out.push_str("CASE");
                if let Some(x) = subject {
                    out.push(' ');
                    self.expr_into(x, 0, out);
                }
                for w in whens {
                    out.push_str(" WHEN ");
                    self.expr_into(&w.cond, 0, out);
                    out.push_str(" THEN ");
                    self.expr_into(&w.then, 0, out);
                }
                if let Some(x) = else_ {
                    out.push_str(" ELSE ");
                    self.expr_into(x, 0, out);
                }
                out.push_str(" END");
            }
            // Operators are written by `expr_into` around their first operand.
            _ => {}
        }
    }

    fn sub(&self, sub: &Sub) -> String {
        match sub {
            Sub::Patterns { patterns, where_ } => {
                let mut s = self.patterns(patterns);
                if let Some(w) = where_ {
                    s.push_str(" WHERE ");
                    s.push_str(&self.expr(w, 0));
                }
                s
            }
            Sub::Clauses { clauses, ret } => {
                let mut parts: Vec<String> =
                    clauses.iter().map(|c| self.clause_inline(c)).collect();
                if let Some(r) = ret {
                    parts.push(self.ret(r));
                }
                parts.join(" ")
            }
        }
    }

    // ----- transactions --------------------------------------------------------------------------------------------

    fn tx(&self, t: &Tx) -> String {
        let mut head = String::from("TX");
        if let Some(r) = &t.on {
            head.push_str(&format!(" ON {}", self.rev(r)));
        }
        if let Some(r) = &t.if_tip {
            head.push_str(&format!(" IF TIP {}", self.rev(r)));
        }
        if let Some(s) = &t.if_targets {
            head.push_str(&format!(" IF TARGETS {}", string_lit(s)));
        }
        if let Some(s) = &t.key {
            head.push_str(&format!(" KEY {}", string_lit(s)));
        }
        if let Some(s) = &t.lease {
            head.push_str(&format!(" LEASE {}", string_lit(s)));
        }
        if let Some(s) = &t.message {
            head.push_str(&format!(" MESSAGE {}", string_lit(s)));
        }
        head.push_str(" {");
        let mut lines = vec![head];
        let n = t.stmts.len();
        for (i, s) in t.stmts.iter().enumerate() {
            let mut line = format!("  {}", self.stmt(s));
            if i + 1 < n {
                line.push(';');
            }
            lines.push(line);
        }
        lines.push(if t.dry {
            "} DRY".to_string()
        } else {
            "}".to_string()
        });
        lines.join("\n")
    }

    fn target(&self, t: &Target) -> String {
        match &t.kind {
            TargetKind::Ident(n) => var(n),
            TargetKind::Nid(n) => format!("#{n}"),
            TargetKind::Uid(h) => format!("#u:{h}"),
            TargetKind::Param(p) => format!("${p}"),
        }
    }

    fn expect(&self, e: &Expect) -> String {
        match e {
            Expect::Exact(n) => n.to_string(),
            Expect::Range(a, b) => format!("{a}..{b}"),
            Expect::Le(n) => format!("<= {n}"),
            Expect::Ge(n) => format!(">= {n}"),
            Expect::Param(p) => format!("${}", p.text),
        }
    }

    fn edge_step(&self, dir: EdgeDir, ty: &Name, props: &[Kv]) -> String {
        let mut body = format!(":{}", plain(&ty.text));
        if !props.is_empty() {
            body.push(' ');
            body.push_str(&self.props(props));
        }
        match dir {
            EdgeDir::Right => format!("-[{body}]->"),
            EdgeDir::Left => format!("<-[{body}]-"),
        }
    }

    fn mutation(&self, m: &Mut) -> String {
        match m {
            Mut::Set(assigns, _) => format!(
                "SET {}",
                assigns
                    .iter()
                    .map(|a| format!(
                        "{}.{} = {}",
                        self.target(&a.target),
                        plain(&a.prop.text),
                        self.expr(&a.value, 0)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Mut::Remove(items, _) => format!(
                "REMOVE {}",
                items
                    .iter()
                    .map(|t| format!("{}.{}", self.target(&t.target), plain(&t.prop.text)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Mut::Delete { targets, opts, .. } => {
                let mut s = format!(
                    "DELETE {}",
                    targets
                        .iter()
                        .map(|t| self.target(t))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                for o in opts {
                    s.push(' ');
                    s.push_str(&match o {
                        DOpt::Policy(Policy::Restrict) => "POLICY RESTRICT".to_string(),
                        DOpt::Policy(Policy::Cascade) => "POLICY CASCADE".to_string(),
                        DOpt::Policy(Policy::Reparent) => "POLICY REPARENT".to_string(),
                        DOpt::Replaced(t) => format!("REPLACED BY {}", self.target(t)),
                        DOpt::Release => "RELEASE".to_string(),
                        DOpt::Reason(e) => format!("REASON {}", self.expr(e, 0)),
                    });
                }
                s
            }
            Mut::Move {
                target, under, pos, ..
            } => {
                let mut s = format!("MOVE {} UNDER {}", self.target(target), self.target(under));
                match pos {
                    None => {}
                    Some(MovePos::Before(t)) => s.push_str(&format!(" BEFORE {}", self.target(t))),
                    Some(MovePos::After(t)) => s.push_str(&format!(" AFTER {}", self.target(t))),
                    Some(MovePos::First) => s.push_str(" FIRST"),
                    Some(MovePos::Last) => s.push_str(" LAST"),
                }
                s
            }
            Mut::Edge(e) => format!(
                "CREATE ({}){}({})",
                self.target(&e.src),
                self.edge_step(e.dir, &e.ty, &e.props),
                self.target(&e.dst)
            ),
            Mut::Reopen { target, reason, .. } => format!(
                "REOPEN {} REASON {}",
                self.target(target),
                self.expr(reason, 0)
            ),
            Mut::Patch {
                target,
                field,
                remove,
                add,
                ..
            } => format!(
                "PATCH {}.{} REMOVE {} ADD {}",
                self.target(target),
                plain(&field.text),
                self.expr(remove, 0),
                self.expr(add, 0)
            ),
        }
    }

    fn muts(&self, muts: &[Mut]) -> String {
        muts.iter()
            .map(|m| self.mutation(m))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// One `TX` statement on one line.
    pub fn stmt(&self, s: &Stmt) -> String {
        match s {
            Stmt::Match(m) => {
                let mut t = format!("MATCH {}", self.patterns(&m.patterns));
                if let Some(w) = &m.where_ {
                    t.push_str(&format!(" WHERE {}", self.expr(w, 0)));
                }
                t.push_str(&format!(
                    " EXPECT {} {}",
                    self.expect(&m.expect),
                    self.muts(&m.muts)
                ));
                t
            }
            Stmt::Muts(muts, _) => self.muts(muts),
            Stmt::Create(c) => {
                let mut t = format!("CREATE ({}:{}", var(&c.var.text), plain(&c.label.text));
                if !c.props.is_empty() {
                    t.push(' ');
                    t.push_str(&self.props(&c.props));
                }
                t.push(')');
                for e in &c.edges {
                    t.push_str(&self.edge_step(e.dir, &e.ty, &e.props));
                    t.push_str(&format!("({})", self.target(&e.target)));
                }
                if let Some(u) = &c.under {
                    t.push_str(&format!(" UNDER {}", self.target(u)));
                }
                if let Some(sub) = &c.unless {
                    t.push_str(&format!(" UNLESS EXISTS {{ {} }}", self.sub(sub)));
                }
                t
            }
            Stmt::TxCall {
                name, args, yield_, ..
            } => {
                let mut t = format!("CALL tx.{}({})", plain(&name.text), self.args(args));
                if !yield_.is_empty() {
                    t.push_str(&format!(" YIELD {}", self.yields(yield_)));
                }
                t
            }
            Stmt::Assert { expr, else_, .. } => {
                let mut t = format!("ASSERT {}", self.expr(expr, 0));
                if let Some(e) = else_ {
                    t.push_str(&format!(" ELSE {}", string_lit(e)));
                }
                t
            }
            Stmt::Resolve(r) => {
                let what = match &r.what {
                    ResolveWhat::Key(k) => string_lit(k),
                    ResolveWhat::Query(q, e) => {
                        format!("({}) EXPECT {}", self.query_inline(q), self.expect(e))
                    }
                };
                let take = match &r.take {
                    Take::Ours => "OURS".to_string(),
                    Take::Theirs => "THEIRS".to_string(),
                    Take::Base => "BASE".to_string(),
                    Take::Value(e) => format!("VALUE {}", self.expr(e, 0)),
                    Take::Repoint(t) => format!("REPOINT {}", self.target(t)),
                };
                format!("RESOLVE {what} TAKE {take}")
            }
            Stmt::Define(d) => format!(
                "{} {{ {} }}",
                self.define_header(d, false),
                self.query_inline(&d.body)
            ),
            Stmt::Drop(n) => format!("DROP QUERY {}", qname(&n.text)),
        }
    }

    /// `DEFINE QUERY name(params) [SHAPE s] [BUDGET b] AS`; with `wrap`, a parameter list over 100 bytes breaks after
    /// a comma with continuation lines aligned under the first parameter ([LQ/gql-spelling §5.2]).
    fn define_header(&self, d: &Define, wrap: bool) -> String {
        let prefix = format!("DEFINE QUERY {}(", qname(&d.name.text));
        let params: Vec<String> = d.params.iter().map(|p| self.pdecl(p)).collect();
        let joined = params.join(", ");
        let list = if wrap && joined.len() > 100 {
            let pad = " ".repeat(prefix.len());
            let mut lines: Vec<String> = Vec::new();
            let mut cur = String::new();
            for (i, p) in params.iter().enumerate() {
                let piece = if i + 1 < params.len() {
                    format!("{p},")
                } else {
                    p.clone()
                };
                if !cur.is_empty() && cur.len() + 1 + piece.len() > 100 {
                    lines.push(cur);
                    cur = piece;
                } else {
                    if !cur.is_empty() {
                        cur.push(' ');
                    }
                    cur.push_str(&piece);
                }
            }
            lines.push(cur);
            lines.join(&format!("\n{pad}"))
        } else {
            joined
        };
        let mut s = format!("{prefix}{list})");
        if let Some(sh) = &d.shape {
            s.push_str(&format!(" SHAPE {}", plain(&sh.text)));
        }
        if let Some(b) = &d.budget {
            s.push_str(&format!(" BUDGET {}", plain(&b.text)));
        }
        s.push_str(" AS");
        s
    }

    fn pdecl(&self, p: &PDecl) -> String {
        let mut s = format!("${}: {}", p.name.text, plain(&p.ty.name.text));
        if let Some(a) = &p.ty.arg {
            s.push_str(&format!("<{}>", plain(&a.text)));
        }
        if p.optional {
            s.push('?');
        }
        if let Some(d) = &p.default {
            s.push_str(&format!(" = {}", self.expr(d, 0)));
        }
        s
    }
}

fn qname(s: &str) -> String {
    s.split('.').map(plain).collect::<Vec<_>>().join(".")
}

/// Whether an argument's text would start with a name and `:` and so read as a named argument
/// ([LQ/grammar-v1.ebnf §P.10]; `arg = [ ident ':' ] …` admits a back-quoted name too). The text's start is found by
/// walking first operands in a loop.
fn starts_with_word_colon(e: &Expr) -> bool {
    let mut e = e;
    loop {
        match &e.kind {
            ExprKind::LabelTest(x, _) => {
                if prints_as_word(x) || matches!(x.kind, ExprKind::Ident(_)) {
                    return true;
                }
                e = x;
            }
            ExprKind::Or(l, _)
            | ExprKind::And(l, _)
            | ExprKind::Cmp(_, l, _)
            | ExprKind::In(l, _)
            | ExprKind::StrPred(_, l, _)
            | ExprKind::Arith(_, l, _)
            | ExprKind::IsNull(_, l) => e = l,
            _ => return false,
        }
    }
}

/// Whether an expression prints as one word token: an unquoted identifier, `TRUE`, `FALSE` or `NULL`.
fn prints_as_word(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(n) => var(n) == *n,
        ExprKind::Bool(_) | ExprKind::Null => true,
        _ => false,
    }
}

/// Whether an expression's text starts with `word IN`.
fn starts_with_word_in(e: &Expr) -> bool {
    let mut e = e;
    loop {
        match &e.kind {
            ExprKind::In(l, _) => {
                if prints_as_word(l) {
                    return true;
                }
                e = l;
            }
            ExprKind::Or(l, _) | ExprKind::And(l, _) => e = l,
            _ => return false,
        }
    }
}

/// The precedence level of an expression node (see [`Printer::expr`]).
fn prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Or(..) => 1,
        ExprKind::And(..) => 2,
        ExprKind::Not(..) => 3,
        ExprKind::Cmp(..)
        | ExprKind::IsNull(..)
        | ExprKind::In(..)
        | ExprKind::StrPred(..)
        | ExprKind::LabelTest(..) => 4,
        ExprKind::Arith(ArithOp::Add | ArithOp::Sub, ..) => 5,
        ExprKind::Arith(..) => 6,
        ExprKind::Neg(..) => 7,
        ExprKind::Prop(..) => 8,
        _ => 9,
    }
}

/// The least precedence an operator's first operand prints at without parentheses (see [`Printer::expr`]).
fn first_min(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Or(..) => 1,
        ExprKind::And(..) => 2,
        ExprKind::Not(..) => 3,
        ExprKind::Cmp(..)
        | ExprKind::IsNull(..)
        | ExprKind::In(..)
        | ExprKind::StrPred(..)
        | ExprKind::LabelTest(..)
        | ExprKind::Arith(ArithOp::Add | ArithOp::Sub, ..) => 5,
        ExprKind::Arith(..) => 6,
        ExprKind::Neg(..) | ExprKind::Prop(..) => 8,
        _ => 0,
    }
}
