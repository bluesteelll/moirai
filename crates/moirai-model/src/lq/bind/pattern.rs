//! Patterns ([LQ/canonical-ast §5.4], §5.7 V2–V3): variables, kind sets ([50 §3.2] "Kinds of a variable"), edge
//! types and directions, the direction typing of E106, quantified groups, and the reading echo of [LQ/envelope §4].

use super::{BKind, Binder};
use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::Ty;
use crate::lq::ctx::{Profile, Value};
use crate::lq::diag::{Code, Diag, Span, list, near, q};
use crate::lq::printer::{self, cypher_quant, gql_quant};
use crate::lq::schema::{DELETED_BIT, KindSet};

/// The binding of each element of a path, in pattern order.
struct Slots {
    start: BindingId,
    steps: Vec<(Option<BindingId>, BindingId)>,
}

/// One resolved edge type: (edge-kind index, written as a forward name).
pub(super) type Resolved = Vec<(usize, bool)>;

/// The effective direction of a type ([LQ/canonical-ast §5.4]): 1 right, 2 left, 3 both.
pub(super) fn effective(written: Dir, forward: bool, symmetric: bool) -> u8 {
    if symmetric {
        return 3;
    }
    match (written, forward) {
        (Dir::Right, true) | (Dir::Left, false) => 1,
        (Dir::Left, true) | (Dir::Right, false) => 2,
        (Dir::Both, _) => 3,
    }
}

impl Binder<'_> {
    /// Binds the patterns of a `MATCH`, `OPTIONAL MATCH`, `TX` `MATCH` or pattern subquery, then its `WHERE`. `top`
    /// marks a clause of a query part (its quantified parts feed N08).
    pub(super) fn match_patterns(
        &mut self,
        paths: &[Path],
        where_: Option<&Expr>,
        top: bool,
    ) -> (Vec<CPath>, Option<CExpr>) {
        let out = self.paths(paths, top);
        let w = where_.map(|e| self.bool_expr(e, super::AggPos::Where));
        (out, w)
    }

    /// Passes 1–3 over a list of paths: declare the variables, type the edges, build the C-AST.
    fn paths(&mut self, paths: &[Path], top: bool) -> Vec<CPath> {
        let slots: Vec<Slots> = paths.iter().map(|p| self.declare_path(p)).collect();
        for (p, s) in paths.iter().zip(&slots) {
            self.type_edges(p, s);
        }
        let mut out = Vec::with_capacity(paths.len());
        for (p, s) in paths.iter().zip(&slots) {
            if top && self.quant_part.is_none() {
                let mut left = s.start;
                for (step, (_, right)) in p.steps.iter().zip(&s.steps) {
                    let quantified = match step {
                        Step::Edge(e, _) => e.quant.is_some(),
                        Step::Group(..) => true,
                    };
                    if quantified {
                        self.quant_part = Some((
                            self.b[left as usize].display.clone(),
                            self.b[*right as usize].display.clone(),
                        ));
                        break;
                    }
                    left = *right;
                }
            }
            out.push(self.build_path(p, s));
        }
        out
    }

    // ----- pass 1: variables, labels, literal ids -----------------------------------------------------------------

    fn declare_path(&mut self, p: &Path) -> Slots {
        let start = self.node_slot(&p.start);
        let mut steps = Vec::with_capacity(p.steps.len());
        for s in &p.steps {
            match s {
                Step::Edge(e, n) => {
                    let ev = self.edge_slot(e);
                    let nv = self.node_slot(n);
                    steps.push((ev, nv));
                }
                Step::Group(_, n) => {
                    let nv = self.node_slot(n);
                    steps.push((None, nv));
                }
            }
        }
        Slots { start, steps }
    }

    fn node_slot(&mut self, n: &NPat) -> BindingId {
        let (id, fresh) = match &n.var {
            Some(v) => match self.lookup(&v.text) {
                Some(id) => {
                    let (kind, ty) = (self.b[id as usize].kind, self.b[id as usize].ty.clone());
                    if kind == BKind::Edge || !matches!(ty, Ty::Node(_) | Ty::Any) {
                        let d = Diag::new(
                            Code::E103,
                            v.span,
                            format!("{} is a bound {}, not a node", q(&v.text), ty.name()),
                        );
                        self.err(d);
                    }
                    if ty == Ty::Any {
                        self.b[id as usize].ty = Ty::Node(self.all_kinds());
                    }
                    (id, false)
                }
                None => (
                    self.declare(&v.text, Ty::Node(self.all_kinds()), BKind::Node),
                    true,
                ),
            },
            None => (
                self.new_binding("", Ty::Node(self.all_kinds()), BKind::Node),
                true,
            ),
        };
        // The pattern's kinds: its labels (DELETED only by its label, [50 §3.6]) and a literal id's kind. A fresh binding
        // takes them (live kinds when unlabelled); a joined one intersects them with what it has.
        let mut kinds = if n.labels.is_empty() {
            None
        } else {
            self.labels(&n.labels).0
        };
        if let Some(k) = self.literal_kind(n) {
            kinds = Some(kinds.map_or(k, |ks| ks.and(k)));
        }
        if let Ty::Node(cur) = self.b[id as usize].ty {
            let next = match (fresh, kinds) {
                (true, Some(k)) => k,
                (true, None) => self.all_kinds(),
                (false, Some(k)) => cur.and(k),
                (false, None) => cur,
            };
            self.b[id as usize].ty = Ty::Node(next);
        }
        let display = self.node_display(n);
        if n.var.is_none() || self.b[id as usize].display.is_empty() {
            self.b[id as usize].display = display;
        }
        id
    }

    fn edge_slot(&mut self, e: &EPat) -> Option<BindingId> {
        let v = e.var.as_ref()?;
        let id = match self.lookup(&v.text) {
            Some(id) if self.b[id as usize].kind == BKind::Edge => id,
            Some(id) => {
                let d = Diag::new(
                    Code::E103,
                    v.span,
                    format!(
                        "{} is a bound {}, not an edge",
                        q(&v.text),
                        self.b[id as usize].ty.name()
                    ),
                );
                self.err(d);
                id
            }
            None => self.declare(
                &v.text,
                Ty::Edge(KindSet::first(self.ctx.schema.edges.len())),
                BKind::Edge,
            ),
        };
        if let Some(qn) = e.quant {
            self.b[id as usize].quant_edge = true;
            let t = e.types.first().map_or("T".to_string(), |t| t.text.clone());
            let m = qn.max.map_or(String::new(), |m| m.to_string());
            self.err(
                Diag::new(
                    Code::E113,
                    v.span,
                    format!(
                        "{} names a quantified edge; LQ has no path or list values",
                        q(&v.text)
                    ),
                )
                .inline(format!(
                    "write a quantified group: (x)((a)-[{}:{t}]->(b) WHERE ...){{{},{m}}}(y)",
                    printer::var(&v.text),
                    qn.min
                )),
            );
        }
        Some(id)
    }

    /// Resolves labels: kind names ASCII case-insensitively, and `DELETED` ([LQ/canonical-ast §5.3]); E105 for an unknown
    /// label. Returns the kind set (`None` after an error) and the canonical names.
    pub(super) fn labels(&mut self, labels: &[Name]) -> (Option<KindSet>, Vec<String>) {
        let mut set = KindSet::EMPTY;
        let mut names = Vec::with_capacity(labels.len());
        let mut ok = true;
        for l in labels {
            if l.text.eq_ignore_ascii_case("deleted") {
                set.insert(DELETED_BIT);
                names.push("DELETED".to_string());
            } else if let Some(k) = self.ctx.schema.kind(&l.text) {
                set.insert(k);
                names.push(self.ctx.schema.kinds[k].name.clone());
            } else {
                ok = false;
                let kinds: Vec<&str> = self
                    .ctx
                    .schema
                    .kinds
                    .iter()
                    .map(|k| k.name.as_str())
                    .collect();
                let s = near(&l.text, kinds.iter().copied());
                let mut d = Diag::new(Code::E105, l.span, format!("unknown kind {}", q(&l.text)))
                    .help(format!("kinds: {}", kinds.join(" ")));
                if let Some(first) = s.first() {
                    d = d.inline(format!("did you mean {}?", q(first)));
                }
                d.suggest = s.into();
                self.err(d);
            }
        }
        (ok.then_some(set), names)
    }

    /// The kind of a node pattern anchored by a literal id (`(#51)`, `{id: 40}`, `{uid: '...'}`), when the store knows it.
    fn literal_kind(&self, n: &NPat) -> Option<KindSet> {
        let uid = self.anchor_uid(n)?;
        let kind = self.ctx.ids.kind_of(&uid)?;
        self.ctx.schema.kind(kind).map(KindSet::one)
    }

    fn anchor_uid(&self, n: &NPat) -> Option<Uid> {
        let kv = n
            .props
            .iter()
            .find(|kv| kv.key.text == "id" || kv.key.text == "uid")?;
        match &kv.value.kind {
            ExprKind::Nid(n) => self.ctx.ids.uid(*n),
            ExprKind::Int(n) if kv.key.text == "id" => {
                u32::try_from(*n).ok().and_then(|n| self.ctx.ids.uid(n))
            }
            ExprKind::Uid(h) => super::value::parse_uid(h),
            ExprKind::Str(h) if kv.key.text == "uid" => super::value::parse_uid(h),
            ExprKind::Param(p) => match self.ctx.params.get(p) {
                Some(Value::Int(n)) => u32::try_from(*n).ok().and_then(|n| self.ctx.ids.uid(n)),
                Some(Value::Text(t)) => super::value::node_text(t).and_then(|r| match r {
                    super::value::NodeRef::Num(n) => self.ctx.ids.uid(n),
                    super::value::NodeRef::Uid(u) => Some(u),
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether an endpoint is anchored ([LQ/envelope §4.1]): a node literal, or an `id` or `uid` property whose value is
    /// a literal or a parameter.
    fn anchored(n: &NPat) -> bool {
        n.props.iter().any(|kv| {
            (kv.key.text == "id" || kv.key.text == "uid")
                && matches!(
                    kv.value.kind,
                    ExprKind::Nid(_)
                        | ExprKind::Uid(_)
                        | ExprKind::Int(_)
                        | ExprKind::Str(_)
                        | ExprKind::Param(_)
                )
        })
    }

    /// An endpoint as the echo prints it ([LQ/envelope §4.3]).
    fn node_display(&self, n: &NPat) -> String {
        if Self::anchored(n) {
            let kv = n
                .props
                .iter()
                .find(|kv| kv.key.text == "id" || kv.key.text == "uid")
                .map(|kv| &kv.value.kind);
            let local = |u: Uid| {
                self.ctx.ids.nid(&u).map_or_else(
                    || format!("#u:{}", super::value::hex(&u)),
                    |n| format!("#{n}"),
                )
            };
            return match kv {
                Some(ExprKind::Nid(n)) => format!("#{n}"),
                Some(ExprKind::Int(n)) => format!("#{n}"),
                Some(ExprKind::Uid(h)) => {
                    super::value::parse_uid(h).map_or_else(|| format!("#u:{h}"), local)
                }
                Some(ExprKind::Str(h)) => {
                    super::value::parse_uid(h).map_or_else(|| h.clone(), local)
                }
                Some(ExprKind::Param(p)) => match self.anchor_uid(n) {
                    Some(u) => local(u),
                    None => format!("${p}"),
                },
                _ => "()".to_string(),
            };
        }
        if let Some(v) = &n.var {
            return v.text.clone();
        }
        if n.labels.is_empty() {
            "()".to_string()
        } else {
            let names: Vec<String> = n
                .labels
                .iter()
                .map(|l| {
                    self.ctx
                        .schema
                        .kind(&l.text)
                        .map_or_else(|| l.text.clone(), |k| self.ctx.schema.kinds[k].name.clone())
                })
                .collect();
            format!("(:{})", names.join("|"))
        }
    }

    // ----- pass 2: edge types, directions, kind sets --------------------------------------------------------------

    /// Resolves the written type names of an edge pattern ([LQ/canonical-ast §5.4]); E107 and E104.
    pub(super) fn edge_types(&mut self, types: &[Name]) -> Option<Resolved> {
        let mut out = Vec::with_capacity(types.len());
        let mut ok = true;
        for t in types {
            if t.text.eq_ignore_ascii_case("parent") {
                ok = false;
                self.err(
                    Diag::new(Code::E107, t.span, "parent is ambiguous in a pattern")
                        .help("write (child)-[:CHILD_OF]->(parent), (parent)-[:PARENT_OF]->(child) or the property n.parent"),
                );
                continue;
            }
            match self.ctx.schema.edge_name(&t.text) {
                Some(r) => out.push((r.edge, r.forward)),
                None => {
                    ok = false;
                    let names = self.ctx.schema.edge_spellings();
                    let s = near(&t.text, names.iter().map(String::as_str));
                    let mut d = Diag::new(
                        Code::E104,
                        t.span,
                        format!("unknown edge type {}", q(&t.text)),
                    );
                    if let Some(first) = s.first() {
                        d = d.inline(format!("did you mean {}?", q(first)));
                    }
                    d.suggest = s.into();
                    self.err(d);
                }
            }
        }
        ok.then_some(out)
    }

    /// The endpoint kinds a set of types admits: (left, right) for the written direction.
    pub(super) fn admitted(&self, types: &Resolved, dir: Dir) -> (KindSet, KindSet) {
        let s = self.ctx.schema;
        let mut l = KindSet::EMPTY;
        let mut r = KindSet::EMPTY;
        for &(e, fwd) in types {
            let d = &s.edges[e];
            let (src, dst) = (s.ends(&d.src), s.ends(&d.dst));
            match effective(dir, fwd, d.symmetric) {
                1 => {
                    l = l.or(src);
                    r = r.or(dst);
                }
                2 => {
                    l = l.or(dst);
                    r = r.or(src);
                }
                _ => {
                    l = l.or(src).or(dst);
                    r = r.or(src).or(dst);
                }
            }
        }
        // A tombstone keeps the kind it had; historical edges into it and flagged edges out of it are traversable
        // through a `:DELETED` node pattern ([50 §3.6]), so `DELETED` is admitted at both ends.
        l.insert(DELETED_BIT);
        r.insert(DELETED_BIT);
        (l, r)
    }

    fn type_edges(&mut self, p: &Path, s: &Slots) {
        let mut left_n = &p.start;
        let mut left = s.start;
        for (step, &(ev, right)) in p.steps.iter().zip(&s.steps) {
            if let Step::Edge(e, right_n) = step {
                if let Some(types) = self.edge_types(&e.types)
                    && !types.is_empty()
                {
                    if let Some(ev) = ev {
                        let mut set = KindSet::EMPTY;
                        for &(k, _) in &types {
                            set.insert(k);
                        }
                        if let Ty::Edge(cur) = self.b[ev as usize].ty {
                            self.b[ev as usize].ty = Ty::Edge(cur.and(set));
                        }
                    }
                    self.record_blocks(&types, e.dir, left, right);
                    if e.quant.is_none_or(|q| q.min > 0) {
                        self.constrain(e, &types, (left_n, left), (right_n, right));
                    }
                }
                left_n = right_n;
            } else if let Step::Group(_, n) = step {
                left_n = n;
            }
            left = right;
        }
    }

    /// Notes the sources of `BLOCKS`/`GATES` edges, for W07.
    fn record_blocks(&mut self, types: &Resolved, dir: Dir, left: BindingId, right: BindingId) {
        for &(k, fwd) in types {
            let d = &self.ctx.schema.edges[k];
            if d.stored != "blocks" && d.stored != "gates" {
                continue;
            }
            let (src, dst) = match effective(dir, fwd, d.symmetric) {
                1 => (left, right),
                2 => (right, left),
                _ => continue,
            };
            let dst_name = self.b[dst as usize].name.clone();
            let name = if dst_name.is_empty() {
                self.b[src as usize].name.clone()
            } else {
                dst_name
            };
            self.blocks_sources.push((src, name));
        }
    }

    /// Intersects the endpoint kind sets with what the types admit; E106 when one becomes empty ([50 §3.2]).
    fn constrain(
        &mut self,
        e: &EPat,
        types: &Resolved,
        (ln, l): (&NPat, BindingId),
        (rn, r): (&NPat, BindingId),
    ) {
        let (al, ar) = self.admitted(types, e.dir);
        let (kl, kr) = match (&self.b[l as usize].ty, &self.b[r as usize].ty) {
            (Ty::Node(a), Ty::Node(b)) => (*a, *b),
            _ => return,
        };
        let (nl, nr) = (kl.and(al), kr.and(ar));
        if (nl.is_empty() && !kl.is_empty()) || (nr.is_empty() && !kr.is_empty()) {
            self.direction_error(e, types, (ln, kl), (rn, kr));
            return;
        }
        self.b[l as usize].ty = Ty::Node(nl);
        self.b[r as usize].ty = Ty::Node(nr);
    }

    fn direction_error(
        &mut self,
        e: &EPat,
        types: &Resolved,
        (ln, kl): (&NPat, KindSet),
        (rn, kr): (&NPat, KindSet),
    ) {
        let s = self.ctx.schema;
        let a = self.node_display(ln);
        let b = self.node_display(rn);
        let task = s.kind("task").map(KindSet::one);
        let only_depends = types
            .iter()
            .all(|&(k, _)| s.edges[k].stored == "depends_on");
        if only_depends && Some(kl) == task && Some(kr) == task {
            self.err(
                Diag::new(Code::E106, e.span, format!("DEPENDS_ON links doc sections (doc -> doc); {} and {} are tasks", q(&a), q(&b)))
                    .inline("between tasks write (d)-[:BLOCKS]->(t) (d finishes before t) or (t)-[:BLOCKED_BY]->(d)"),
            );
            return;
        }
        let names: Vec<&str> = types.iter().map(|&(k, _)| s.edges[k].lq.as_str()).collect();
        let (k0, fwd0) = types[0];
        let d0 = &s.edges[k0];
        let (src, dst) = if fwd0 {
            (&d0.src, &d0.dst)
        } else {
            (&d0.dst, &d0.src)
        };
        let msg = format!(
            "{} links {} -> {}; {} and {} are {} and {}",
            q(&names.join("|")),
            s.kinds_text(s.ends(src)),
            s.kinds_text(s.ends(dst)),
            q(&a),
            q(&b),
            s.kinds_text(kl),
            s.kinds_text(kr)
        );
        let mut d = Diag::new(Code::E106, e.span, msg);
        // The reversed pattern, when it types.
        let rev_dir = match e.dir {
            Dir::Right => Some(Dir::Left),
            Dir::Left => Some(Dir::Right),
            Dir::Both => None,
        };
        if let Some(rd) = rev_dir {
            let (al, ar) = self.admitted(types, rd);
            if !kl.and(al).is_empty() && !kr.and(ar).is_empty() {
                let mut rev = e.clone();
                rev.dir = rd;
                let path = Path {
                    start: ln.clone(),
                    steps: vec![Step::Edge(rev, rn.clone())],
                    span: Span::default(),
                };
                d = d.inline(format!("write {}", printer::path_text(&path)));
            }
        }
        let connecting = self.connecting(kl, kr);
        if !connecting.is_empty() {
            d = d.help(format!(
                "edges from {} to {}: {}",
                s.kinds_text(kl),
                s.kinds_text(kr),
                list(&connecting)
            ));
        }
        self.err(d);
    }

    /// The type names (LQ names and reverse aliases) that connect two kind sets.
    fn connecting(&self, from: KindSet, to: KindSet) -> Vec<String> {
        let s = self.ctx.schema;
        let mut out = Vec::new();
        for d in &s.edges {
            let (src, dst) = (s.ends(&d.src), s.ends(&d.dst));
            if !from.and(src).is_empty() && !to.and(dst).is_empty() {
                out.push(d.lq.clone());
            }
            if !from.and(dst).is_empty() && !to.and(src).is_empty() {
                out.extend(d.reverse.iter().cloned());
            }
        }
        out
    }

    // ----- pass 3: the C-AST, property maps, WHERE, the echo ------------------------------------------------------

    fn build_path(&mut self, p: &Path, s: &Slots) -> CPath {
        let start = self.build_node(&p.start, s.start);
        let mut steps = Vec::with_capacity(p.steps.len());
        let mut left_n = &p.start;
        for (step, &(ev, right)) in p.steps.iter().zip(&s.steps) {
            match step {
                Step::Edge(e, n) => {
                    let ce = self.build_edge(e, ev);
                    self.echo_edge(e, left_n, n);
                    let cn = self.build_node(n, right);
                    steps.push(CStep::Edge(ce, cn));
                    left_n = n;
                }
                Step::Group(g, n) => {
                    let cg = self.group(g, left_n, n);
                    let cn = self.build_node(n, right);
                    steps.push(CStep::Group(cg, cn));
                    left_n = n;
                }
            }
        }
        CPath { start, steps }
    }

    fn build_node(&mut self, n: &NPat, id: BindingId) -> CNode {
        let labels = if n.labels.is_empty() {
            Vec::new()
        } else {
            n.labels
                .iter()
                .map(|l| {
                    if l.text.eq_ignore_ascii_case("deleted") {
                        "DELETED".to_string()
                    } else {
                        self.ctx.schema.kind(&l.text).map_or_else(
                            || l.text.clone(),
                            |k| self.ctx.schema.kinds[k].name.clone(),
                        )
                    }
                })
                .collect()
        };
        let base = self.b[id as usize].ty.clone();
        let mut props = Vec::with_capacity(n.props.len());
        for kv in &n.props {
            let (pty, _) = self.prop_type(
                &base,
                &kv.key.text,
                kv.key.span,
                self.b[id as usize].name.clone(),
            );
            let (c, t) = self.expr_at(&kv.value, Some(&pty), super::AggPos::Other);
            self.check_assignable(&pty, &t, &kv.key.text, kv.value.span);
            props.push((kv.key.text.clone(), c));
        }
        let where_ = n
            .where_
            .as_ref()
            .map(|w| self.bool_expr(w, super::AggPos::Where));
        CNode {
            var: n.var.as_ref().map(|_| id),
            labels,
            props,
            where_,
        }
    }

    fn build_edge(&mut self, e: &EPat, ev: Option<BindingId>) -> CEdgeP {
        let s = self.ctx.schema;
        let mut types = Vec::with_capacity(e.types.len());
        for t in &e.types {
            if t.text.eq_ignore_ascii_case("parent") {
                continue;
            }
            if let Some(r) = s.edge_name(&t.text) {
                let d = &s.edges[r.edge];
                types.push((d.lq.clone(), effective(e.dir, r.forward, d.symmetric)));
            }
        }
        let dir = if e.types.is_empty() {
            match e.dir {
                Dir::Right => 1,
                Dir::Left => 2,
                Dir::Both => 3,
            }
        } else {
            0
        };
        let base = ev.map_or(Ty::Edge(KindSet::first(s.edges.len())), |v| {
            self.b[v as usize].ty.clone()
        });
        let mut props = Vec::with_capacity(e.props.len());
        for kv in &e.props {
            let owner = e.var.as_ref().map_or_else(String::new, |v| v.text.clone());
            let (pty, _) = self.prop_type(&base, &kv.key.text, kv.key.span, owner);
            let (c, t) = self.expr_at(&kv.value, Some(&pty), super::AggPos::Other);
            self.check_assignable(&pty, &t, &kv.key.text, kv.value.span);
            if kv.key.text == "anchor"
                && self.def.is_some()
                && matches!(kv.value.kind, ExprKind::Str(_))
            {
                self.anchor_handle(kv.value.span);
            }
            props.push((kv.key.text.clone(), c));
        }
        let where_ = e
            .where_
            .as_ref()
            .map(|w| self.bool_expr(w, super::AggPos::Where));
        CEdgeP {
            var: ev,
            dir,
            types,
            quant: e.quant.map(|q| (q.min, q.max)),
            props,
            where_,
        }
    }

    /// A quantified group (V3): its variables are local to its path and `WHERE`; outside they are E116.
    ///
    /// The group's echo line stands for the edges inside it ([LQ/envelope §4.3]: a group of one edge prints as that edge
    /// between the group's outer endpoints, a group of several edges as `<a> (<T1> <T2> ...)<q> <b>`), so they print no
    /// line of their own ([50 §2.9] Q5 echoes the group alone). Under `gated` a group of several edges prints no line
    /// (see [`Self::echo_group`]) and its edges echo by the `gated` rule.
    fn group(&mut self, g: &Group, left: &NPat, right: &NPat) -> CGroup {
        let mark = self.scope.len();
        let one_edge = matches!(g.path.steps.as_slice(), [Step::Edge(..)]);
        let own_line = self.ctx.caller.profile != Profile::Gated || one_edge;
        let outer = self.mute_echo;
        self.mute_echo = outer || own_line;
        let path = self
            .paths(std::slice::from_ref(&g.path), false)
            .pop()
            .unwrap_or(CPath {
                start: CNode {
                    var: None,
                    labels: Vec::new(),
                    props: Vec::new(),
                    where_: None,
                },
                steps: Vec::new(),
            });
        let where_ = g
            .where_
            .as_ref()
            .map(|w| self.bool_expr(w, super::AggPos::Where));
        let local: Vec<String> = self
            .scope
            .get(mark..)
            .unwrap_or(&[])
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        self.scope.truncate(mark);
        for n in local {
            if !self.steps.contains(&n) {
                self.steps.push(n);
            }
        }
        self.mute_echo = outer;
        if !outer {
            self.echo_group(g, left, right);
        }
        CGroup {
            path,
            where_,
            quant: (g.quant.min, g.quant.max),
        }
    }

    // ----- the reading echo ---------------------------------------------------------------------------------------

    /// `<q>` of the echo's display ([LQ/envelope §4.3]): the quantifier in the caller's display spelling, empty for a
    /// single hop — none, or `{1,1}` ([LQ/gql-spelling §2.1]).
    fn quant_text(&self, q: Option<Quant>) -> String {
        match q {
            None
            | Some(Quant {
                min: 1,
                max: Some(1),
            }) => String::new(),
            Some(q) => match self.ctx.caller.display {
                printer::Spelling::Gql => gql_quant(q),
                printer::Spelling::Cypher => cypher_quant(q),
            },
        }
    }

    fn quant_suffix(q: Option<Quant>) -> String {
        match q {
            None => String::new(),
            Some(Quant {
                min: 1,
                max: Some(1),
            }) => String::new(),
            Some(Quant {
                min,
                max: Some(max),
            }) if min == max => format!(" (through exactly {min} steps)"),
            Some(Quant {
                min,
                max: Some(max),
            }) => format!(" (through {min} to {max} steps)"),
            Some(Quant { min, max: None }) => format!(" (through {min} or more steps)"),
        }
    }

    fn reading(&self, e: usize, a: &str, b: &str) -> String {
        self.ctx.schema.edges[e]
            .reading
            .replace("{a}", a)
            .replace("{b}", b)
    }

    fn push_read(&mut self, line: String) {
        if !self.reads.contains(&line) {
            self.reads.push(line);
        }
    }

    /// Whether an edge pattern echoes under the caller's profile ([LQ/envelope §4.1]).
    fn echoes(&self, types: &Resolved, anchored: bool) -> bool {
        match self.ctx.caller.profile {
            Profile::Compatible | Profile::Unknown => true,
            Profile::Gated => {
                types.iter().any(|&(_, fwd)| !fwd)
                    || (anchored
                        && types.iter().any(|&(k, _)| {
                            self.ctx.schema.same_kind_edge(k) && !self.ctx.schema.edges[k].symmetric
                        }))
            }
        }
    }

    fn echo_edge(&mut self, e: &EPat, ln: &NPat, rn: &NPat) {
        if self.mute_echo {
            return;
        }
        let Some(types) = self.silent_types(&e.types) else {
            return;
        };
        if types.is_empty() || !self.echoes(&types, Self::anchored(ln) || Self::anchored(rn)) {
            return;
        }
        let a = self.node_display(ln);
        let b = self.node_display(rn);
        let line = self.echo_line(&types, e.dir, &e.types, e.quant, &a, &b);
        self.push_read(line);
    }

    /// Resolves type names without diagnostics (pass 2 reported them).
    fn silent_types(&self, types: &[Name]) -> Option<Resolved> {
        types
            .iter()
            .map(|t| {
                if t.text.eq_ignore_ascii_case("parent") {
                    None
                } else {
                    self.ctx
                        .schema
                        .edge_name(&t.text)
                        .map(|r| (r.edge, r.forward))
                }
            })
            .collect()
    }

    fn echo_line(
        &self,
        types: &Resolved,
        dir: Dir,
        written: &[Name],
        quant: Option<Quant>,
        a: &str,
        b: &str,
    ) -> String {
        let s = self.ctx.schema;
        let q = self.quant_text(quant);
        let effs: Vec<u8> = types
            .iter()
            .map(|&(k, fwd)| effective(dir, fwd, s.edges[k].symmetric))
            .collect();
        let mut names: Vec<&str> = Vec::new();
        for &(k, _) in types {
            if !names.contains(&s.edges[k].lq.as_str()) {
                names.push(&s.edges[k].lq);
            }
        }
        let names = names.join("|");
        let uniform = effs.iter().all(|&d| d == effs[0]);
        let (display, reading) = if uniform && effs[0] == 3 {
            if dir == Dir::Both {
                let r: Vec<String> = types
                    .iter()
                    .map(|&(k, _)| {
                        format!("{}, or {}", self.reading(k, a, b), self.reading(k, b, a))
                    })
                    .collect();
                (
                    format!("{a} {names}{q} {b} (either direction)"),
                    r.join(", or "),
                )
            } else {
                let r: Vec<String> = types.iter().map(|&(k, _)| self.reading(k, a, b)).collect();
                (format!("{a} {names}{q} {b}"), r.join(", or "))
            }
        } else if uniform {
            let (x, y) = if effs[0] == 1 { (a, b) } else { (b, a) };
            let r: Vec<String> = types.iter().map(|&(k, _)| self.reading(k, x, y)).collect();
            (format!("{x} {names}{q} {y}"), r.join(", or "))
        } else {
            let r: Vec<String> = types
                .iter()
                .zip(&effs)
                .map(|(&(k, _), &d)| {
                    if d == 2 {
                        self.reading(k, b, a)
                    } else {
                        self.reading(k, a, b)
                    }
                })
                .collect();
            (format!("{a} {names}{q} {b}"), r.join(", or "))
        };
        let mut line = display;
        if types.iter().any(|&(_, fwd)| !fwd) {
            let written_names: Vec<String> = written
                .iter()
                .map(|w| {
                    s.edges
                        .iter()
                        .flat_map(|d| d.reverse.iter().chain(std::iter::once(&d.lq)))
                        .find(|n| n.eq_ignore_ascii_case(&w.text))
                        .cloned()
                        .unwrap_or_else(|| w.text.to_ascii_uppercase())
                })
                .collect();
            let (x, y) = if dir == Dir::Left { (b, a) } else { (a, b) };
            line.push_str(&format!(
                " (written {x} {}{q} {y})",
                written_names.join("|")
            ));
        }
        format!("{line} | {reading}{}", Self::quant_suffix(quant))
    }

    /// A quantified group's line ([LQ/envelope §4.1], §4.3). A group of one edge prints as that edge between the
    /// group's outer endpoints, and under `gated` it echoes as that edge would: when an outer endpoint is anchored and
    /// its kind is same-kind, or when it is written with a reverse alias ([50 §2.9] Q5 echoes
    /// `(x:task)((a:task)-[:BLOCKS]->(b) WHERE a.unfinished){1,5}(#93)`). A group of several edges is no hop, so
    /// `gated` leaves it to its edges.
    fn echo_group(&mut self, g: &Group, ln: &NPat, rn: &NPat) {
        let gated = self.ctx.caller.profile == Profile::Gated;
        if let [Step::Edge(e, _)] = g.path.steps.as_slice() {
            if let Some(types) = self.silent_types(&e.types)
                && !types.is_empty()
                && (!gated || self.echoes(&types, Self::anchored(ln) || Self::anchored(rn)))
            {
                let a = self.node_display(ln);
                let b = self.node_display(rn);
                let line = self.echo_line(&types, e.dir, &e.types, Some(g.quant), &a, &b);
                self.push_read(line);
            }
            return;
        }
        if gated {
            return;
        }
        let a = self.node_display(ln);
        let b = self.node_display(rn);
        let mut names = Vec::new();
        let mut stack: Vec<&Path> = vec![&g.path];
        while let Some(p) = stack.pop() {
            for st in &p.steps {
                match st {
                    Step::Edge(e, _) => {
                        let n: Vec<String> = self
                            .silent_types(&e.types)
                            .unwrap_or_default()
                            .iter()
                            .map(|&(k, _)| self.ctx.schema.edges[k].lq.clone())
                            .collect();
                        names.push(if n.is_empty() {
                            "()".to_string()
                        } else {
                            n.join("|")
                        });
                    }
                    Step::Group(inner, _) => stack.push(&inner.path),
                }
            }
        }
        let q = self.quant_text(Some(g.quant));
        let line = format!(
            "{a} ({}){q} {b} | {a} reaches {b} through {}{}",
            names.join(" "),
            names.join(" then "),
            Self::quant_suffix(Some(g.quant))
        );
        self.push_read(line);
    }

    /// E117: an anchor handle compared or bound in a definition ([LQ/canonical-ast §8.1] step 5).
    pub(super) fn anchor_handle(&mut self, span: Span) {
        self.err(
            Diag::new(
                Code::E117,
                span,
                "a reflog position or anchor handle differs per store",
            )
            .help("use a $param, a c<hex> commit id, a ref name or the anchor's fields"),
        );
    }
}
