//! Expressions ([50 §3.3]; [LQ/canonical-ast §6.3]): the two-valued logic with an explicit absent value (or, under
//! ablation D8, SQL/GQL three-valued logic), comparisons and membership, arithmetic with E103 on overflow and absent
//! (notice N10) on a division by zero, the scalar built-ins, subqueries, list predicates and `CASE`.

use super::func;
use super::val::{self, EnumV, V};
use super::view::Ev;
use crate::err::{Refusal, Res};
use crate::lq::cast::{BindingId, CArg, CExpr};
use crate::value::Nid;
use std::cmp::Ordering;

/// One binding row: the values of the bindings by binding id, and the binding identity of [50 §3.5] (every element
/// bound so far, anonymous ones included, in order of first appearance).
#[derive(Clone, Debug, Default)]
pub struct Row {
    /// Values by binding id; an unset id is absent.
    pub vals: Vec<V>,
    /// Which binding ids are bound (a binding an `OPTIONAL MATCH` left absent is bound, to the absent value).
    pub bound: Vec<bool>,
    /// The binding identity.
    pub ident: Vec<V>,
}

impl Row {
    /// The value of a binding.
    pub fn get(&self, b: BindingId) -> &V {
        self.vals.get(b as usize).unwrap_or(&V::Absent)
    }

    /// Whether a binding is bound.
    pub fn is_bound(&self, b: BindingId) -> bool {
        self.bound.get(b as usize).copied().unwrap_or(false)
    }

    /// Sets a binding.
    pub fn set(&mut self, b: BindingId, v: V) {
        let i = b as usize;
        if self.vals.len() <= i {
            self.vals.resize(i + 1, V::Absent);
            self.bound.resize(i + 1, false);
        }
        self.vals[i] = v;
        self.bound[i] = true;
    }
}

/// What one evaluation reads besides the expression: the row, the parameters of the enclosing definition, the items of
/// the `RETURN` its sort key belongs to (`ITEMREF`), and the rows of the group an aggregate reads.
#[derive(Clone, Copy)]
pub struct Env<'e> {
    /// The binding row.
    pub row: &'e Row,
    /// `PARAM(i)` values.
    pub params: &'e [V],
    /// `ITEMREF(i)` values.
    pub items: Option<&'e [V]>,
    /// The group's rows, for aggregates.
    pub group: Option<&'e [Row]>,
}

impl<'e> Env<'e> {
    /// An environment of one row.
    pub fn of(row: &'e Row, params: &'e [V]) -> Env<'e> {
        Env {
            row,
            params,
            items: None,
            group: None,
        }
    }
}

/// Facts about the evaluation of one row that the counted warnings and notices need ([LQ/errors §5.6]).
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// The fields whose absence decided an ordered comparison (W01).
    pub absent: Vec<String>,
    /// A `<>` or membership test read `link_state()` of an unlinked node, `'none'` (W10).
    pub none_link: bool,
    /// A division by zero gave absent (N10).
    pub div0: bool,
}

/// The truth of a predicate value: `true` only for `true` ([50 §3.3]: `WHERE` keeps the rows whose predicate holds).
pub fn truth(v: &V) -> bool {
    matches!(v, V::Bool(true))
}

/// A float result with `-0.0` normalised to `0.0`, as every arithmetic operator gives it ([50 §3.3]).
fn float(x: f64) -> V {
    V::Float(if x == 0.0 { 0.0 } else { x })
}

impl Ev<'_> {
    /// Runs `f` as a nested evaluation ([LQ/errors §5.6]: the counted warnings and notices count the rows of the
    /// outermost evaluation): the facts of the enclosing row are kept, and the nested evaluation's own are neither
    /// counted nor merged into them.
    pub fn nested<T>(&self, f: impl FnOnce() -> Res<T>) -> Res<T> {
        let saved = std::mem::take(&mut *self.flags.borrow_mut());
        let hits = self.hits.borrow_mut().take();
        self.depth.set(self.depth.get() + 1);
        let r = f();
        self.depth.set(self.depth.get() - 1);
        *self.flags.borrow_mut() = saved;
        *self.hits.borrow_mut() = hits;
        r
    }

    /// Whether the evaluation is the outermost one, whose rows the counted warnings and notices count.
    pub fn counting(&self) -> bool {
        self.depth.get() == 0
    }

    /// Evaluates an expression.
    // spec: [50 §3.3]
    pub fn eval(&self, e: &CExpr, env: &Env<'_>) -> Res<V> {
        let tri = self.w.ab.three_valued;
        Ok(match e {
            CExpr::Or(a, b) => {
                let x = self.eval(a, env)?;
                if truth(&x) {
                    return Ok(V::Bool(true));
                }
                let y = self.eval(b, env)?;
                if tri && (x.is_absent() || y.is_absent()) && !truth(&y) {
                    V::Absent
                } else {
                    V::Bool(truth(&y))
                }
            }
            CExpr::And(a, b) => {
                let x = self.eval(a, env)?;
                if matches!(x, V::Bool(false)) || (!tri && !truth(&x)) {
                    return Ok(V::Bool(false));
                }
                let y = self.eval(b, env)?;
                if matches!(y, V::Bool(false)) {
                    V::Bool(false)
                } else if tri && (x.is_absent() || y.is_absent()) {
                    V::Absent
                } else {
                    V::Bool(truth(&y))
                }
            }
            CExpr::Not(a) => match self.eval(a, env)? {
                V::Absent if tri => V::Absent,
                v => V::Bool(!truth(&v)),
            },
            CExpr::Cmp(op, a, b) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                self.cmp(*op, a, b, &x, &y)
            }
            CExpr::IsNull(neg, a) => {
                let x = self.eval(a, env)?;
                V::Bool(x.is_absent() != *neg)
            }
            CExpr::In(a, b) => {
                let x = self.eval(a, env)?;
                let l = self.eval(b, env)?;
                self.note_none_link(a, &x);
                self.member(&x, &l)
            }
            CExpr::StrPred(op, a, b) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                match (x.as_str(), y.as_str()) {
                    (Some(s), Some(t)) => V::Bool(match op {
                        1 => s.starts_with(t),
                        2 => s.ends_with(t),
                        _ => s.contains(t),
                    }),
                    _ if tri => V::Absent,
                    _ => V::Bool(false),
                }
            }
            CExpr::LabelTest(a, labels) => match self.eval(a, env)? {
                V::Node(n) => V::Bool(self.node(n).is_some_and(|x| {
                    labels.iter().any(|l| {
                        if l == "DELETED" {
                            x.tomb.is_some()
                        } else {
                            x.kind.eq_ignore_ascii_case(l)
                        }
                    })
                })),
                V::Absent if tri => V::Absent,
                _ => V::Bool(false),
            },
            CExpr::Arith(op, a, b) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                self.arith(*op, &x, &y)?
            }
            CExpr::Neg(a) => match self.eval(a, env)? {
                V::Int(i) => V::Int(
                    i.checked_neg()
                        .ok_or_else(|| Refusal::lq("E103", "integer overflow in -"))?,
                ),
                V::Float(f) => V::Float(if f == 0.0 { 0.0 } else { -f }),
                V::Dur(d) => V::Dur(
                    d.checked_neg()
                        .ok_or_else(|| Refusal::lq("E103", "duration overflow in -"))?,
                ),
                _ => V::Absent,
            },
            CExpr::Prop(a, name) => match self.eval(a, env)? {
                V::Node(n) => self.prop(n, name)?,
                V::Edge(x) => self.edge_prop(&x, name)?,
                m @ V::Map(_) => m.member(name),
                _ => V::Absent,
            },
            CExpr::Var(b) => env.row.get(*b).clone(),
            CExpr::Param(i) => env.params.get(*i as usize).cloned().unwrap_or(V::Absent),
            CExpr::ItemRef(i) => env
                .items
                .and_then(|it| it.get(*i as usize).cloned())
                .unwrap_or(V::Absent),
            CExpr::Exists(s) => V::Bool(self.sub_count(s, env, true)? > 0),
            CExpr::CountSub(s) => V::Int(self.sub_count(s, env, false)? as i64),
            CExpr::Func(name, distinct, args) => self.func(name, *distinct, args, env)?,
            CExpr::CountStar => V::Int(env.group.map_or(1, |g| g.len()) as i64),
            CExpr::ListPred(kind, var, list, p) => {
                let l = self.eval(list, env)?;
                if l.is_absent() {
                    return Ok(if tri { V::Absent } else { V::Bool(false) });
                }
                let (mut t, mut f, mut u) = (0usize, 0usize, 0usize);
                for x in l.elems() {
                    let mut r = env.row.clone();
                    r.set(*var, x.clone());
                    let v = self.eval(p, &Env { row: &r, ..*env })?;
                    if truth(&v) {
                        t += 1;
                    } else if v.is_absent() && tri {
                        u += 1;
                    } else {
                        f += 1;
                    }
                }
                // all: no false; any: some true; none: no true. An unknown element decides only when nothing else does.
                let r = match kind {
                    1 if f > 0 => Some(false),
                    2 | 3 if t > 0 => Some(*kind == 2),
                    _ if u > 0 => None,
                    1 | 3 => Some(true),
                    _ => Some(false),
                };
                r.map_or(V::Absent, V::Bool)
            }
            CExpr::List(v) => V::List(
                v.iter()
                    .map(|x| self.eval(x, env))
                    .collect::<Res<Vec<V>>>()?,
            ),
            CExpr::Map(m) => V::Map(
                m.iter()
                    .map(|(k, x)| Ok((k.clone(), self.eval(x, env)?)))
                    .collect::<Res<Vec<_>>>()?,
            ),
            CExpr::Case(subject, arms, other) => {
                let s = subject.as_ref().map(|s| self.eval(s, env)).transpose()?;
                for (w, t) in arms {
                    let wv = self.eval(w, env)?;
                    let hit = match &s {
                        Some(s) => val::eq(s, &wv) == Some(true),
                        None => truth(&wv),
                    };
                    if hit {
                        return self.eval(t, env);
                    }
                }
                match other {
                    Some(o) => self.eval(o, env)?,
                    None => V::Absent,
                }
            }
            CExpr::Null => V::Absent,
            CExpr::Bool(b) => V::Bool(*b),
            CExpr::Int(i) => V::Int(*i),
            CExpr::Float(f) => V::Float(*f),
            CExpr::Text(s) => V::text(s.clone()),
            CExpr::Duration(d) => V::Dur(*d),
            CExpr::Timestamp(t) => V::Time(*t),
            CExpr::Node(u) => self
                .store()
                .alloc
                .uidx
                .get(&crate::value::Uid(*u))
                .map_or(V::Absent, |n| V::Node(*n)),
            // An enum constant is a value of its field, ranked there like the field's stored values ([50 §3.5]).
            CExpr::Enum(s, of) => self.enum_v(&of.kind, &of.field, s),
            CExpr::RangeInt(a, b) => V::Range(*a, *b),
            CExpr::RHead | CExpr::RRef(_) | CExpr::RCommit(_) | CExpr::RSuf(..) => {
                match self.w.commit_of(e)? {
                    Some(c) => V::Rev(c),
                    None => V::Absent,
                }
            }
            CExpr::RList(v) => V::List(
                v.iter()
                    .map(|x| self.eval(x, env))
                    .collect::<Res<Vec<V>>>()?,
            ),
            CExpr::RRange(..) => return Err(Refusal::usage("a revision range is not a value")),
        })
    }

    /// Records W10's fact: a membership or `<>` test over `link_state()` of an unlinked node read `'none'`.
    fn note_none_link(&self, a: &CExpr, x: &V) {
        if matches!(a, CExpr::Func(f, _, _) if f == "link_state") && x.as_str() == Some("none") {
            self.flags.borrow_mut().none_link = true;
        }
    }

    /// Records W01's fact: an ordered comparison of a property that read absent.
    fn note_absent(&self, a: &CExpr, x: &V) {
        if let (CExpr::Prop(_, name), V::Absent) = (a, x) {
            let mut f = self.flags.borrow_mut();
            if !f.absent.contains(name) {
                f.absent.push(name.clone());
            }
        }
    }

    /// A comparison ([50 §3.3]): with an absent operand `=` and the ordered operators are false and `<>` true (under
    /// three-valued logic, unknown). Numbers compare numerically, an int equal to a float included. An ordered operator
    /// between an enumeration and a text ranks the text in the enumeration's own field, whatever expression gave either
    /// operand ([50 §3.5]); a text that is no value of that field makes the comparison false.
    // spec: [50 §3.3] comparison
    // spec: [50 §3.5] enumerations
    fn cmp(&self, op: u8, a: &CExpr, b: &CExpr, x: &V, y: &V) -> V {
        let tri = self.w.ab.three_valued;
        if x.is_absent() || y.is_absent() {
            if op >= 3 {
                self.note_absent(a, x);
                self.note_absent(b, y);
            }
            if tri {
                return V::Absent;
            }
            return V::Bool(op == 2);
        }
        if op == 2 {
            self.note_none_link(a, x);
            self.note_none_link(b, y);
        }
        match op {
            1 => V::Bool(val::eq(x, y) == Some(true)),
            2 => V::Bool(val::eq(x, y) != Some(true)),
            _ => match self.ord_ranked(x, y) {
                None => V::Bool(false),
                Some(o) => V::Bool(match op {
                    3 => o == Ordering::Less,
                    4 => o != Ordering::Greater,
                    5 => o == Ordering::Greater,
                    _ => o != Ordering::Less,
                }),
            },
        }
    }

    /// The order of two present operands for an ordered operator, an enumeration against a text by rank ([50 §3.5]).
    fn ord_ranked(&self, x: &V, y: &V) -> Option<Ordering> {
        let ranked = |e: &EnumV, t: &str| self.rank_text(e, t);
        match (x, y) {
            (V::Enum(e), V::Text(t)) => ranked(e, t).map(|r| val::cmp_total(x, &r)),
            (V::Text(t), V::Enum(e)) => ranked(e, t).map(|r| val::cmp_total(&r, y)),
            _ => val::ord(x, y),
        }
    }

    /// `x IN l` ([50 §3.3]): false for an absent `x`; a list membership by `=`; an integer range by its bounds.
    // spec: [50 §3.3] membership
    fn member(&self, x: &V, l: &V) -> V {
        let tri = self.w.ab.three_valued;
        if x.is_absent() || l.is_absent() {
            return if tri { V::Absent } else { V::Bool(false) };
        }
        if let V::Range(lo, hi) = l {
            return V::Bool(match x {
                V::Int(i) => lo.is_none_or(|a| *i >= a) && hi.is_none_or(|b| *i <= b),
                _ => false,
            });
        }
        let mut unknown = false;
        for e in l.elems() {
            match val::eq(x, e) {
                Some(true) => return V::Bool(true),
                None => unknown = true,
                _ => {}
            }
        }
        if unknown && tri {
            V::Absent
        } else {
            V::Bool(false)
        }
    }

    /// Arithmetic ([50 §3.3]; [LQ/canonical-ast §5.10]): integers stay integers (overflow is E103), `/` is a float and
    /// a division by zero is absent (N10); durations and timestamps combine as §5.10 states; absent operands give absent.
    // spec: [50 §3.3] arithmetic
    // spec: [LQ/canonical-ast §5.10]
    fn arith(&self, op: u8, x: &V, y: &V) -> Res<V> {
        let ovf = || Refusal::lq("E103", "integer overflow");
        let f = |v: &V| match v {
            V::Int(i) => Some(*i as f64),
            V::Float(f) => Some(*f),
            _ => None,
        };
        Ok(match (op, x, y) {
            (_, V::Absent, _) | (_, _, V::Absent) => V::Absent,
            (4, _, _) => match (f(x), f(y)) {
                (Some(_), Some(0.0)) => {
                    self.flags.borrow_mut().div0 = true;
                    V::Absent
                }
                (Some(a), Some(b)) if (a / b).is_finite() => float(a / b),
                (Some(_), Some(_)) => return Err(Refusal::lq("E103", "float overflow in /")),
                _ => V::Absent,
            },
            (1, V::Int(a), V::Int(b)) => V::Int(a.checked_add(*b).ok_or_else(ovf)?),
            (2, V::Int(a), V::Int(b)) => V::Int(a.checked_sub(*b).ok_or_else(ovf)?),
            (3, V::Int(a), V::Int(b)) => V::Int(a.checked_mul(*b).ok_or_else(ovf)?),
            (1, V::Time(t), V::Dur(d)) | (1, V::Dur(d), V::Time(t)) => {
                V::Time(t.checked_add(*d).ok_or_else(ovf)?)
            }
            (2, V::Time(t), V::Dur(d)) => V::Time(t.checked_sub(*d).ok_or_else(ovf)?),
            (2, V::Time(a), V::Time(b)) => V::Dur(a.checked_sub(*b).ok_or_else(ovf)?),
            (1, V::Dur(a), V::Dur(b)) => V::Dur(a.checked_add(*b).ok_or_else(ovf)?),
            (2, V::Dur(a), V::Dur(b)) => V::Dur(a.checked_sub(*b).ok_or_else(ovf)?),
            (3, V::Int(a), V::Dur(b)) | (3, V::Dur(b), V::Int(a)) => {
                V::Dur(a.checked_mul(*b).ok_or_else(ovf)?)
            }
            (1, V::Text(a), V::Text(b)) => V::text(format!("{a}{b}")),
            (1, V::List(a), V::List(b)) => V::List(a.iter().chain(b).cloned().collect()),
            (_, _, _) => match (f(x), f(y)) {
                (Some(a), Some(b)) => {
                    let r = match op {
                        1 => a + b,
                        2 => a - b,
                        _ => a * b,
                    };
                    if r.is_finite() {
                        float(r)
                    } else {
                        return Err(Refusal::lq("E103", "float overflow"));
                    }
                }
                _ => return Err(Refusal::lq("E103", "the operand types do not combine")),
            },
        })
    }

    /// The node of an argument value, if it holds one.
    fn arg_node(&self, a: &[CArg], i: usize, env: &Env<'_>) -> Res<Option<Nid>> {
        Ok(match a.get(i) {
            Some(x) => self.eval(&x.value, env)?.node(),
            None => None,
        })
    }

    /// A scalar or aggregate built-in ([LQ/canonical-ast] Table 5.3; [LQ/std §2.10]).
    // spec: [LQ/std §2.10]
    fn func(&self, name: &str, distinct: bool, args: &[CArg], env: &Env<'_>) -> Res<V> {
        if matches!(name, "count" | "sum" | "min" | "max" | "avg" | "collect") {
            return self.aggregate(name, distinct, args, env);
        }
        let arg = |i: usize| -> Res<V> {
            match args.get(i) {
                Some(a) => self.eval(&a.value, env),
                None => Ok(V::Absent),
            }
        };
        let nodes = |v: Vec<Nid>| V::List(v.into_iter().map(V::Node).collect());
        Ok(match name {
            "subtree" | "descendants" => match self.arg_node(args, 0, env)? {
                Some(n) => nodes(func::subtree(self, n, arg(1)?.as_int(), name == "subtree")),
                None => V::Absent,
            },
            "ancestors" => self
                .arg_node(args, 0, env)?
                .map_or(V::Absent, |n| nodes(func::ancestors(self, n))),
            "children" => self
                .arg_node(args, 0, env)?
                .map_or(V::Absent, |n| nodes(func::children(self, n))),
            "applies" | "applies_role" | "applies_phase" | "fits_role" => {
                let (Some(n), V::Text(t)) = (self.arg_node(args, 0, env)?, arg(1)?) else {
                    return Ok(V::Bool(false));
                };
                V::Bool(match name {
                    "applies" => func::applies(self, n, &t),
                    "applies_role" => func::applies_tag(self, n, "role", &t),
                    "applies_phase" => func::applies_tag(self, n, "phase", &t),
                    _ => func::fits_role(self, n, &t),
                })
            }
            "glob_match" => match (arg(0)?, arg(1)?) {
                (V::Text(p), V::Text(g)) => V::Bool(crate::r4::path::glob_match(&g, &p)),
                _ => V::Bool(false),
            },
            "text_match" => match (self.arg_node(args, 0, env)?, arg(1)?) {
                (Some(n), V::Text(t)) => V::Bool(super::search::text_match(self, n, &t)),
                _ => V::Bool(false),
            },
            "file" => {
                let root = match arg(1)? {
                    V::Text(r) => r,
                    _ => "project".into(),
                };
                match arg(0)? {
                    V::Text(p) => super::rel::file_of(self, &root, &p),
                    _ => V::Absent,
                }
            }
            "link_state" => match arg(0)? {
                V::Node(n) => {
                    if !self.at_tip() {
                        return Err(self.not_here("link_state()", "the file tree"));
                    }
                    V::text(super::rel::link_state_node(self, n)?)
                }
                V::Edge(e) => {
                    if !self.at_tip() {
                        return Err(self.not_here("link_state()", "the file tree"));
                    }
                    super::rel::link_state_edge(self, &e)?.map_or(V::Absent, V::text)
                }
                _ => V::Absent,
            },
            "staleness" => match self.arg_node(args, 0, env)? {
                Some(n) => {
                    if !self.at_tip() {
                        return Err(self.not_here("staleness()", "git ancestry"));
                    }
                    V::text(super::rel::staleness(self, n))
                }
                None => V::Absent,
            },
            "relevant_to" => match (self.arg_node(args, 0, env)?, arg(1)?) {
                (n, V::Text(a)) => V::Bool(crate::feed::relevant_to(
                    self.st(),
                    self.w.lease_table(),
                    n,
                    &a,
                )),
                _ => V::Bool(false),
            },
            "me" => V::text(self.w.caller.actor.clone()),
            "view_ref" => V::text(self.v.ref_name.clone()),
            "now" => V::Time(self.now()),
            "datetime" | "date" => match arg(0)? {
                V::Text(s) => func::parse_time(&s),
                V::Time(t) => V::Time(t),
                V::Absent if args.is_empty() => V::Time(self.now()),
                _ => V::Absent,
            },
            "duration" => match arg(0)? {
                V::Text(s) => func::parse_duration(&s),
                V::Dur(d) => V::Dur(d),
                _ => V::Absent,
            },
            "size" => match arg(0)? {
                V::List(v) => V::Int(v.len() as i64),
                V::Text(s) => V::Int(s.chars().count() as i64),
                V::Absent => V::Absent,
                _ => V::Int(1),
            },
            "lower" => arg(0)?
                .as_str()
                .map_or(V::Absent, |s| V::text(s.to_lowercase())),
            "upper" => arg(0)?
                .as_str()
                .map_or(V::Absent, |s| V::text(s.to_uppercase())),
            "trim" => arg(0)?.as_str().map_or(V::Absent, |s| V::text(s.trim())),
            "substring" => match (arg(0)?, arg(1)?) {
                (V::Text(s), V::Int(start)) => {
                    let len = arg(2)?.as_int();
                    let cs = s.chars().skip(start.max(0) as usize);
                    V::text(match len {
                        Some(l) => cs.take(l.max(0) as usize).collect::<String>(),
                        None => cs.collect::<String>(),
                    })
                }
                _ => V::Absent,
            },
            "coalesce" => {
                for i in 0..args.len() {
                    let v = arg(i)?;
                    if !v.is_absent() {
                        return Ok(v);
                    }
                }
                V::Absent
            }
            "round" => {
                let d = arg(1)?.as_int().unwrap_or(0) as i32;
                match arg(0)? {
                    V::Int(i) if d >= 0 => V::Float(i as f64),
                    V::Int(i) => V::Float(round_away(i as f64, d)),
                    V::Float(x) => V::Float(round_away(x, d)),
                    _ => V::Absent,
                }
            }
            "abs" => match arg(0)? {
                V::Int(i) => V::Int(
                    i.checked_abs()
                        .ok_or_else(|| Refusal::lq("E103", "integer overflow in abs"))?,
                ),
                V::Float(f) => V::Float(f.abs()),
                V::Dur(d) => V::Dur(
                    d.checked_abs()
                        .ok_or_else(|| Refusal::lq("E103", "duration overflow in abs"))?,
                ),
                _ => V::Absent,
            },
            "toString" => match arg(0)? {
                V::Absent => V::Absent,
                v => V::text(display(&v)),
            },
            "toInteger" => match arg(0)? {
                V::Int(i) => V::Int(i),
                V::Float(f) => int_of(f),
                V::Text(s) => s.trim().parse::<i64>().map_or_else(
                    |_| s.trim().parse::<f64>().map_or(V::Absent, int_of),
                    V::Int,
                ),
                V::Bool(b) => V::Int(i64::from(b)),
                _ => V::Absent,
            },
            "toFloat" => match arg(0)? {
                V::Int(i) => V::Float(i as f64),
                V::Float(f) => V::Float(f),
                V::Text(s) => s
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|f| f.is_finite())
                    .map_or(V::Absent, float),
                _ => V::Absent,
            },
            "id" => match arg(0)? {
                n @ V::Node(_) => n,
                _ => V::Absent,
            },
            "labels" => match self.arg_node(args, 0, env)? {
                Some(n) => self
                    .node(n)
                    .map_or(V::Absent, |x| V::List(vec![V::text(x.kind.clone())])),
                None => V::Absent,
            },
            "type" => match arg(0)? {
                V::Edge(e) => V::text(self.lq_name(&e.kind)),
                _ => V::Absent,
            },
            other => {
                return Err(Refusal::lq(
                    "E109",
                    format!("no function {other} in the model"),
                ));
            }
        })
    }

    /// An aggregate over the group's rows ([50 §3.4] rule 6; [50 §3.3]: absent inputs are skipped): `count(x)` counts
    /// present values, `DISTINCT` the distinct ones; `collect` returns its values in the natural order ([50 §3.5]). A
    /// division by zero in the argument marks the input row for N10 ([LQ/errors §5.6]).
    // spec: [50 §3.4] aggregates
    fn aggregate(&self, name: &str, distinct: bool, args: &[CArg], env: &Env<'_>) -> Res<V> {
        let rows: &[Row] = env.group.unwrap_or(std::slice::from_ref(env.row));
        let mut vals = Vec::with_capacity(rows.len());
        if let Some(a) = args.first() {
            for (i, r) in rows.iter().enumerate() {
                let before = std::mem::take(&mut self.flags.borrow_mut().div0);
                let v = self.eval(
                    &a.value,
                    &Env {
                        row: r,
                        group: None,
                        ..*env
                    },
                )?;
                let hit = std::mem::replace(&mut self.flags.borrow_mut().div0, before);
                if hit
                    && let Some(h) = self.hits.borrow_mut().as_mut()
                    && let Some(slot) = h.get_mut(i)
                {
                    *slot = true;
                }
                if !v.is_absent() {
                    vals.push(v);
                }
            }
        }
        if distinct {
            val::sort_natural(&mut vals);
            vals.dedup_by(|a, b| val::same(a, b));
        }
        Ok(match name {
            "count" => V::Int(vals.len() as i64),
            "collect" => {
                val::sort_natural(&mut vals);
                V::List(vals)
            }
            "min" => vals.into_iter().min_by(val::cmp_total).unwrap_or(V::Absent),
            "max" => vals.into_iter().max_by(val::cmp_total).unwrap_or(V::Absent),
            "sum" => {
                if vals.is_empty() {
                    return Ok(V::Int(0));
                }
                let mut acc = V::Int(0);
                for v in &vals {
                    acc = self.arith(1, &acc, v)?;
                }
                acc
            }
            _ => {
                let xs: Vec<f64> = vals
                    .iter()
                    .filter_map(|v| match v {
                        V::Int(i) => Some(*i as f64),
                        V::Float(f) => Some(*f),
                        _ => None,
                    })
                    .collect();
                if xs.is_empty() {
                    V::Absent
                } else {
                    float(xs.iter().sum::<f64>() / xs.len() as f64)
                }
            }
        })
    }

    /// The number of rows of a subquery for the current row ([50 §3.9] item 3: the part's view; [LQ/canonical-ast
    /// §5.7] V8), evaluated as a nested evaluation. `EXISTS` stops at the first row.
    // spec: [LQ/canonical-ast §5.7] V8
    fn sub_count(&self, s: &crate::lq::cast::CSub, env: &Env<'_>, stop: bool) -> Res<usize> {
        use crate::lq::cast::CSub;
        self.nested(|| match s {
            CSub::Patterns(paths, w) => {
                let mut out = Vec::new();
                self.match_paths(env.row, paths, w.as_ref(), env.params, &mut out, stop)?;
                Ok(out.len())
            }
            CSub::Clauses(clauses, ret) => {
                let rows = self.run_clauses(vec![env.row.clone()], clauses, env.params)?;
                match ret {
                    None => Ok(rows.len()),
                    Some(r) => Ok(self.project(rows, r, env.params, None, &[])?.rows.len()),
                }
            }
        })
    }
}

/// `toInteger` of a float ([LQ/std §2.10]): truncated toward zero; absent when it is not finite or its integer part
/// lies outside the int range.
fn int_of(f: f64) -> V {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    let t = f.trunc();
    if t.is_finite() && (-TWO63..TWO63).contains(&t) {
        V::Int(t as i64)
    } else {
        V::Absent
    }
}

/// Rounding at `digits` decimals, half away from zero (`round()`; Cypher's rounding).
fn round_away(x: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    let r = (x * m).round() / m;
    if r == 0.0 { 0.0 } else { r }
}

/// The text of a value for `toString()`.
pub fn display(v: &V) -> String {
    match v {
        V::Absent => String::new(),
        V::Bool(b) => b.to_string(),
        V::Int(i) => i.to_string(),
        V::Float(f) => {
            let s = format!("{f}");
            if s.contains('.') || s.contains('e') || s.contains("inf") {
                s
            } else {
                format!("{s}.0")
            }
        }
        V::Text(s) => s.clone(),
        V::Enum(e) => e.name.clone(),
        V::Time(t) => func::iso(*t),
        V::Dur(d) => format!("{d}ms"),
        V::Node(n) => n.to_string(),
        V::Edge(e) => format!("{}-[:{}]->{}", e.src, e.kind, e.dst),
        V::Rev(s) => format!("s{s}"),
        V::List(v) => format!("[{}]", v.iter().map(display).collect::<Vec<_>>().join(", ")),
        V::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{k}: {}", display(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        V::Range(a, b) => format!(
            "{}..{}",
            a.map_or(String::new(), |x| x.to_string()),
            b.map_or(String::new(), |x| x.to_string())
        ),
    }
}
